use crate::config::save_config;
use eframe::egui;

mod devices;
mod filters;
mod panels;
mod state;
mod tray;
mod widgets;

#[cfg(test)]
mod tests;

pub use state::AppState;
pub(crate) use tray::TrayController;

pub struct MicProcessorApp {
    pub state: AppState,
}

impl MicProcessorApp {
    fn show(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        self.state.meters.decay();
        self.state.toolbar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.heading("Mic Processor");
                ui.label("Microphone → configurable filter chain → Virtual Cable");
                ui.separator();
                self.state.levels_panel(ui);
                self.state.monitor_panel(ui);
                self.state.devices_panel(ui);
                if filters::filter_chain_ui(ui, &mut self.state.config.filters, &mut self.state.next_filter_id) {
                    self.state.params.publish_filters(&self.state.config.filters);
                }
                self.state.startup_panel(ui);
                ui.separator();
                ui.small("Tip: Install VB-Cable, set this app's output to \"CABLE Input\", and set apps \
(Discord/Zoom) to use \"CABLE Output\" as their microphone. Use Mic Test with headphones to verify the chain.");
                ui.add_space(12.0);
            });
        });
    }
}

impl eframe::App for MicProcessorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.state.params.snapshot_into(&mut self.state.config);
        let _ = save_config(&self.state.config);
        self.state.engine.stop();
    }
}
