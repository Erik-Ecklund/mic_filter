use crate::audio::DeviceInfo;
use eframe::egui;

#[derive(Clone, Copy)]
pub(super) enum DeviceRole {
    Input,
    Output,
    Monitor,
}

impl DeviceRole {
    fn id(self) -> &'static str {
        match self {
            Self::Input => "input_dev",
            Self::Output => "output_dev",
            Self::Monitor => "monitor_dev",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Input => "(default)",
            Self::Output => "(auto VB-Cable / default)",
            Self::Monitor => "(auto headphones / default)",
        }
    }

    fn label(self, device: &DeviceInfo) -> String {
        match self {
            Self::Output if device.looks_like_vb_cable => format!("{} ★", device.name),
            _ if device.is_default => format!("{} (default)", device.name),
            Self::Monitor if device.looks_like_vb_cable => format!("{} (virtual)", device.name),
            _ => device.name.clone(),
        }
    }
}

pub(super) fn device_selector(
    ui: &mut egui::Ui,
    role: DeviceRole,
    selected: &mut Option<String>,
    devices: &[DeviceInfo],
) {
    egui::ComboBox::from_id_salt(role.id())
        .selected_text(selected.as_deref().unwrap_or(role.placeholder()))
        .width(360.0)
        .show_ui(ui, |ui| {
            let default_label = match role {
                DeviceRole::Input => "(system default)",
                _ => role.placeholder(),
            };
            ui.selectable_value(selected, None, default_label);
            for device in devices {
                ui.selectable_value(selected, Some(device.name.clone()), role.label(device));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_labels_preserve_role_specific_hints() {
        let device = DeviceInfo {
            name: "Cable".into(),
            is_default: true,
            looks_like_vb_cable: true,
        };
        assert_eq!(DeviceRole::Output.label(&device), "Cable ★");
        assert_eq!(DeviceRole::Input.label(&device), "Cable (default)");
        assert_eq!(DeviceRole::Monitor.label(&device), "Cable (default)");
        assert_eq!(
            DeviceRole::Monitor.label(&DeviceInfo {
                is_default: false,
                ..device
            }),
            "Cable (virtual)"
        );
    }
}
