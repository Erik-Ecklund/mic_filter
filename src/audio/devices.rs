use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub looks_like_vb_cable: bool,
}

pub fn list_input_devices() -> anyhow::Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut out = Vec::new();
    for device in host.input_devices()? {
        let name = device.name().unwrap_or_else(|_| "<unknown>".into());
        let is_default = default_name.as_ref() == Some(&name);
        let looks_like_vb_cable = is_vb_cable_name(&name);
        out.push(DeviceInfo {
            name,
            is_default,
            looks_like_vb_cable,
        });
    }
    Ok(out)
}

pub fn list_output_devices() -> anyhow::Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());
    let mut out = Vec::new();
    for device in host.output_devices()? {
        let name = device.name().unwrap_or_else(|_| "<unknown>".into());
        let is_default = default_name.as_ref() == Some(&name);
        let looks_like_vb_cable = is_vb_cable_name(&name);
        out.push(DeviceInfo {
            name,
            is_default,
            looks_like_vb_cable,
        });
    }
    // Prefer VB-Cable Input near the top for convenience.
    out.sort_by_key(|d| (!d.looks_like_vb_cable, d.name.clone()));
    Ok(out)
}

pub fn find_input_device(preferred: Option<&str>) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = preferred {
        for device in host.input_devices()? {
            if device.name().ok().as_deref() == Some(name) {
                return Ok(device);
            }
        }
        tracing::warn!(%name, "preferred input not found; falling back to default");
    }
    host.default_input_device()
        .ok_or_else(|| anyhow::anyhow!("no default input device"))
}

pub fn find_output_device(preferred: Option<&str>) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = preferred {
        for device in host.output_devices()? {
            if device.name().ok().as_deref() == Some(name) {
                return Ok(device);
            }
        }
        tracing::warn!(%name, "preferred output not found; trying VB-Cable / default");
    }

    // Auto-pick VB-Cable input if present.
    if let Ok(devices) = host.output_devices() {
        for device in devices {
            let Ok(name) = device.name() else {
                continue;
            };
            if is_vb_cable_output_target(&name) {
                tracing::info!(%name, "auto-selected VB-Cable output target");
                return Ok(device);
            }
        }
    }

    host.default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no default output device"))
}

/// Playback device for local mic-test monitoring (prefer real speakers/headphones).
pub fn find_monitor_device(
    preferred: Option<&str>,
    cable_output_name: Option<&str>,
) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = preferred {
        for device in host.output_devices()? {
            if device.name().ok().as_deref() == Some(name) {
                return Ok(device);
            }
        }
        tracing::warn!(%name, "preferred monitor device not found; falling back");
    }

    // Prefer the system default playback device when it is not the virtual cable.
    if let Some(default) = host.default_output_device().filter(|device| {
        device.name().is_ok_and(|name| {
            !is_vb_cable_output_target(&name)
                && !is_vb_cable_name(&name)
                && cable_output_name != Some(name.as_str())
        })
    }) {
        return Ok(default);
    }

    // Otherwise first non-cable output.
    for device in host.output_devices()? {
        if let Ok(name) = device.name() {
            if is_vb_cable_output_target(&name) || is_vb_cable_name(&name) {
                continue;
            }
            if cable_output_name == Some(name.as_str()) {
                continue;
            }
            return Ok(device);
        }
    }

    host.default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no monitor playback device"))
}

pub(crate) fn is_vb_cable_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.contains("cable") && (lower.contains("vb") || lower.contains("virtual")))
        || lower.contains("cable input")
        || lower.contains("cable output")
}

/// Output device we should render *into* (the virtual cable's playback endpoint).
pub(crate) fn is_vb_cable_output_target(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("cable input")
        || (lower.contains("vb-audio") && lower.contains("cable") && !lower.contains("output"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_vb_cable_names() {
        assert!(is_vb_cable_name("CABLE Input (VB-Audio Virtual Cable)"));
        assert!(is_vb_cable_name("CABLE Output (VB-Audio Virtual Cable)"));
        assert!(is_vb_cable_name("VB-Audio Virtual Cable"));
        assert!(!is_vb_cable_name("Microphone (Realtek Audio)"));
        assert!(!is_vb_cable_name("Speakers"));
    }

    #[test]
    fn output_target_prefers_cable_input() {
        assert!(is_vb_cable_output_target(
            "CABLE Input (VB-Audio Virtual Cable)"
        ));
        assert!(is_vb_cable_output_target("VB-Audio Virtual Cable"));
        assert!(!is_vb_cable_output_target(
            "CABLE Output (VB-Audio Virtual Cable)"
        ));
        assert!(!is_vb_cable_output_target("Speakers (Realtek)"));
    }
}
