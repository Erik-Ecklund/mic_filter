use super::AppState;
use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub(crate) struct TrayController {
    _icon: Option<TrayIcon>,
    show: MenuId,
    start: MenuId,
    stop: MenuId,
    mute: MenuId,
    monitor: MenuId,
    quit: MenuId,
}

impl TrayController {
    pub(crate) fn new() -> Self {
        let menu = Menu::new();
        let show = MenuItem::new("Open settings", true, None);
        let start = MenuItem::new("Start", true, None);
        let stop = MenuItem::new("Stop", true, None);
        let mute = MenuItem::new("Toggle mute", true, None);
        let monitor = MenuItem::new("Toggle mic test", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let _ = menu.append_items(&[
            &show,
            &PredefinedMenuItem::separator(),
            &start,
            &stop,
            &mute,
            &monitor,
            &PredefinedMenuItem::separator(),
            &quit,
        ]);
        let (rgba, w, h) = make_tray_icon_rgba();
        let icon = Icon::from_rgba(rgba, w, h).expect("tray icon");
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Mic Processor")
            .with_icon(icon)
            .build()
            .ok();
        Self {
            _icon: icon,
            show: show.id().clone(),
            start: start.id().clone(),
            stop: stop.id().clone(),
            mute: mute.id().clone(),
            monitor: monitor.id().clone(),
            quit: quit.id().clone(),
        }
    }

    pub(crate) fn process_events(&self, state: &mut AppState, ctx: &egui::Context) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.show {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if event.id == self.start {
                state.start_engine();
            } else if event.id == self.stop {
                state.stop_engine();
            } else if event.id == self.mute {
                state.toggle_mute();
            } else if event.id == self.monitor {
                state.toggle_monitor();
            } else if event.id == self.quit {
                state.quit_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

/// Tiny generated tray icon (16x16 teal circle).
fn make_tray_icon_rgba() -> (Vec<u8>, u32, u32) {
    let size = 16u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let cx = 7.5f32;
    let cy = 7.5f32;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let i = ((y * size + x) * 4) as usize;
            if dx * dx + dy * dy <= 6.5 * 6.5 {
                rgba[i] = 0x2E;
                rgba[i + 1] = 0xCC;
                rgba[i + 2] = 0xB0;
                rgba[i + 3] = 0xFF;
            }
        }
    }
    (rgba, size, size)
}
