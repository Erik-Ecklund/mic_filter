use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

use super::{AppConfig, FilterConfig, FilterSettings, LimiterConfig, legacy::normalize_filter_ids};

const MAX_PROFILES: usize = 128;
const MAX_FILTERS: usize = 64;
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub filters: Vec<FilterConfig>,
    #[serde(default)]
    pub limiter: LimiterConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDocument {
    pub version: u32,
    pub profile: Profile,
}

fn check_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "Profile name cannot be empty.");
    ensure!(
        name.chars().count() <= 80,
        "Profile name is limited to 80 characters."
    );
    ensure!(
        !name.chars().any(char::is_control),
        "Profile name cannot contain control characters."
    );
    Ok(())
}

fn check(value: f32, min: f32, max: f32, label: &str) -> Result<()> {
    ensure!(
        value.is_finite() && (min..=max).contains(&value),
        "{label} must be finite and between {min} and {max}."
    );
    Ok(())
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        check_name(&self.name)?;
        ensure!(
            self.filters.len() <= MAX_FILTERS,
            "A profile can contain at most {MAX_FILTERS} filters."
        );
        check(self.limiter.ceiling_db, -24.0, 0.0, "Limiter ceiling")?;
        check(self.limiter.release_ms, 10.0, 1000.0, "Limiter release")?;
        for filter in &self.filters {
            match &filter.settings {
                FilterSettings::HighPass(c) => check(c.cutoff_hz, 40.0, 200.0, "High-pass cutoff")?,
                FilterSettings::Rnnoise(_) => {}
                FilterSettings::Gain(c) => check(c.gain_db, -24.0, 24.0, "Gain")?,
                FilterSettings::Expander(c) => {
                    check(c.threshold_db, -80.0, 0.0, "Expander threshold")?;
                    check(c.ratio, 1.0, 10.0, "Expander ratio")?;
                    check(c.attack_ms, 0.1, 50.0, "Expander attack")?;
                    check(c.release_ms, 10.0, 500.0, "Expander release")?;
                    check(c.hold_ms, 0.0, 300.0, "Expander hold")?;
                    check(c.range_db, 0.0, 60.0, "Expander range")?;
                }
                FilterSettings::Compressor(c) => {
                    check(c.threshold_db, -60.0, 0.0, "Compressor threshold")?;
                    check(c.ratio, 1.0, 20.0, "Compressor ratio")?;
                    check(c.attack_ms, 0.1, 100.0, "Compressor attack")?;
                    check(c.release_ms, 10.0, 500.0, "Compressor release")?;
                    check(c.knee_db, 0.0, 24.0, "Compressor knee")?;
                    check(c.makeup_db, -12.0, 24.0, "Compressor makeup")?;
                }
            }
        }
        Ok(())
    }

    pub fn matches(&self, config: &AppConfig) -> bool {
        self.limiter == config.limiter
            && self.filters.len() == config.filters.len()
            && self
                .filters
                .iter()
                .zip(&config.filters)
                .all(|(a, b)| a.settings == b.settings)
    }
}

impl ProfileDocument {
    pub fn parse(text: &str) -> Result<Profile> {
        ensure!(
            text.len() <= MAX_DOCUMENT_BYTES,
            "Profile import is limited to 1 MiB."
        );
        let mut document: Self = toml::from_str(text)?;
        ensure!(document.version == 1, "Unsupported profile format version.");
        document.profile.name = document.profile.name.trim().to_owned();
        document.profile.validate()?;
        normalize_filter_ids(&mut document.profile.filters);
        Ok(document.profile)
    }

    pub fn export(profile: &Profile) -> Result<String> {
        profile.validate()?;
        Ok(toml::to_string_pretty(&Self {
            version: 1,
            profile: profile.clone(),
        })?)
    }
}

impl AppConfig {
    fn available_profile_name(&self, name: &str, excluding: Option<usize>) -> Result<String> {
        let name = name.trim();
        check_name(name)?;
        ensure!(
            !self
                .profiles
                .iter()
                .enumerate()
                .any(|(i, p)| Some(i) != excluding && p.name.to_lowercase() == name.to_lowercase()),
            "A profile with that name already exists."
        );
        Ok(name.to_owned())
    }

    pub fn add_profile(&mut self, mut profile: Profile) -> Result<usize> {
        ensure!(
            self.profiles.len() < MAX_PROFILES,
            "At most {MAX_PROFILES} profiles can be saved."
        );
        profile.name = self.available_profile_name(&profile.name, None)?;
        profile.validate()?;
        normalize_filter_ids(&mut profile.filters);
        self.profiles.push(profile);
        Ok(self.profiles.len() - 1)
    }

    pub fn save_new_profile(&mut self, name: &str) -> Result<usize> {
        self.add_profile(Profile {
            name: name.to_owned(),
            filters: self.filters.clone(),
            limiter: self.limiter.clone(),
        })
    }

    pub fn overwrite_profile(&mut self, index: usize) -> Result<()> {
        let name = self
            .profiles
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Select a profile."))?
            .name
            .clone();
        let profile = Profile {
            name,
            filters: self.filters.clone(),
            limiter: self.limiter.clone(),
        };
        profile.validate()?;
        self.profiles[index] = profile;
        Ok(())
    }

    pub fn rename_profile(&mut self, index: usize, name: &str) -> Result<()> {
        ensure!(index < self.profiles.len(), "Select a profile.");
        let name = self.available_profile_name(name, Some(index))?;
        self.profiles[index].name = name;
        Ok(())
    }

    pub fn duplicate_profile(&mut self, index: usize, name: &str) -> Result<usize> {
        let mut profile = self
            .profiles
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Select a profile."))?
            .clone();
        profile.name = name.to_owned();
        self.add_profile(profile)
    }

    pub fn delete_profile(&mut self, index: usize) -> Result<()> {
        ensure!(index < self.profiles.len(), "Select a profile.");
        self.profiles.remove(index);
        Ok(())
    }

    pub fn load_profile(&mut self, index: usize, next_id: &mut u64) -> Result<()> {
        let Some(profile) = self.profiles.get(index) else {
            bail!("Select a profile.");
        };
        profile.validate()?;
        let mut filters = profile.filters.clone();
        let mut used: std::collections::HashSet<_> = self.filters.iter().map(|f| f.id).collect();
        // Fresh identities prevent inheriting unrelated envelopes/history.
        for filter in &mut filters {
            while *next_id == 0 || !used.insert(*next_id) {
                *next_id = next_id.wrapping_add(1);
            }
            filter.id = *next_id;
            *next_id = next_id.wrapping_add(1);
        }
        self.filters = filters;
        self.limiter = profile.limiter.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
