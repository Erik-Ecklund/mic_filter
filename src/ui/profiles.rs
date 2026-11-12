use anyhow::{Result, anyhow};
use eframe::egui;

use super::AppState;
use crate::config::ProfileDocument;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct ProfileUi {
    pub selected: Option<String>,
    name: String,
    exchange: String,
    confirm_delete: bool,
}

enum Action {
    SaveNew,
    Load(usize),
    Overwrite(usize),
    Rename(usize),
    Duplicate(usize),
    Delete(usize),
    Import,
    Export(usize),
}

impl AppState {
    /// Shared by the settings UI and tray. Does not change devices or recordings.
    pub(super) fn load_named_profile(&mut self, name: &str) -> Result<()> {
        let index = self
            .config
            .profiles
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| anyhow!("Profile no longer exists."))?;
        self.config.load_profile(index, &mut self.next_filter_id)?;
        self.params
            .publish_profile(&self.config.filters, &self.config.limiter);
        self.profile_ui.selected = Some(name.to_owned());
        self.profile_ui.confirm_delete = false;
        Ok(())
    }

    pub(super) fn switch_profile(&mut self, name: &str) {
        match self.load_named_profile(name) {
            Ok(()) => self.persist(),
            Err(error) => self.status_message = format!("Profile: {error:#}"),
        }
    }

    fn profile_action(&mut self, action: Action) -> Result<bool> {
        let name = self.profile_ui.name.clone();
        let selected = match action {
            Action::SaveNew => Some(self.config.save_new_profile(&name)?),
            Action::Load(index) => {
                let name = self.config.profiles[index].name.clone();
                self.load_named_profile(&name)?;
                None
            }
            Action::Overwrite(index) => {
                self.config.overwrite_profile(index)?;
                None
            }
            Action::Rename(index) => {
                self.config.rename_profile(index, &name)?;
                Some(index)
            }
            Action::Duplicate(index) => Some(self.config.duplicate_profile(index, &name)?),
            Action::Delete(index) => {
                self.config.delete_profile(index)?;
                self.profile_ui.selected = None;
                self.profile_ui.confirm_delete = false;
                None
            }
            Action::Import => Some(
                self.config
                    .add_profile(ProfileDocument::parse(&self.profile_ui.exchange)?)?,
            ),
            Action::Export(index) => {
                self.profile_ui.exchange = ProfileDocument::export(&self.config.profiles[index])?;
                self.status_message =
                    "Profile exported below. Copy the TOML to share or save it.".into();
                return Ok(false);
            }
        };
        if let Some(index) = selected {
            self.profile_ui.selected = Some(self.config.profiles[index].name.clone());
            self.profile_ui.confirm_delete = false;
        }
        Ok(true)
    }

    pub(super) fn profiles_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Named profiles")
            .default_open(true)
            .show(ui, |ui| {
                ui.label("Profiles store the filter chain and final limiter, not devices or recordings.");
                let index = self.profile_ui.selected.as_ref()
                    .and_then(|name| self.config.profiles.iter().position(|p| &p.name == name));
                let mut selected = index;
                egui::ComboBox::from_id_salt("named_profile")
                    .selected_text(index.map_or("Select a profile", |i| self.config.profiles[i].name.as_str()))
                    .show_ui(ui, |ui| {
                        for (i, profile) in self.config.profiles.iter().enumerate() {
                            ui.selectable_value(&mut selected, Some(i), &profile.name);
                        }
                    });
                if selected != index {
                    self.profile_ui.selected = selected.map(|i| self.config.profiles[i].name.clone());
                    self.profile_ui.confirm_delete = false;
                }
                if let Some(index) = selected {
                    if self.config.profiles[index].matches(&self.config) {
                        ui.small("Current settings match this profile.");
                    } else {
                        ui.colored_label(egui::Color32::YELLOW, "Current settings differ from this profile.");
                    }
                }
                ui.small("Load replaces the working chain, including unsaved edits. Save as new first \
                    if needed. Toolbar Save does not overwrite named profiles. Tray switching also replaces edits.");
                ui.horizontal_wrapped(|ui| {
                    ui.label("New / renamed profile name:");
                    ui.add(egui::TextEdit::singleline(&mut self.profile_ui.name)
                        .char_limit(80).hint_text("e.g. Meetings"));
                });
                let mut action = None;
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Save as new").clicked() {
                        action = Some(Action::SaveNew);
                    }
                    if let Some(index) = selected {
                        if ui.button("Load selected").clicked() {
                            action = Some(Action::Load(index));
                        }
                        if ui.button("Overwrite selected").clicked() {
                            action = Some(Action::Overwrite(index));
                        }
                        if ui.button("Rename").clicked() {
                            action = Some(Action::Rename(index));
                        }
                        if ui.button("Duplicate").clicked() {
                            action = Some(Action::Duplicate(index));
                        }
                        if ui.button("Delete…").clicked() {
                            self.profile_ui.confirm_delete = true;
                        }
                    }
                });
                if self.profile_ui.confirm_delete && let Some(index) = selected {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Delete \"{}\"? Working settings remain.", self.config.profiles[index].name));
                        if ui.button("Confirm delete").clicked() {
                            action = Some(Action::Delete(index));
                        }
                        if ui.button("Cancel").clicked() {
                            self.profile_ui.confirm_delete = false;
                        }
                    });
                }
                egui::CollapsingHeader::new("Import / export profile TOML").show(ui, |ui| {
                    ui.label("Export the selected saved profile, or paste a profile document and Import. \
                        Imports create a new profile without loading it; names must be unique.");
                    ui.add(egui::TextEdit::multiline(&mut self.profile_ui.exchange)
                        .code_editor().desired_rows(6).desired_width(f32::INFINITY)
                        .char_limit(1024 * 1024));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Import as new").clicked() {
                            action = Some(Action::Import);
                        }
                        if let Some(index) = selected
                            && ui.button("Export selected").clicked() {
                                action = Some(Action::Export(index));
                        }
                        if ui.button("Copy TOML").clicked() {
                            ui.output_mut(|output| output.copied_text = self.profile_ui.exchange.clone());
                        }
                    });
                });
                if let Some(action) = action {
                    match self.profile_action(action) {
                        Ok(true) => self.persist(),
                        Ok(false) => {}
                        Err(error) => self.status_message = format!("Profile: {error:#}"),
                    }
                }
            });
    }
}
