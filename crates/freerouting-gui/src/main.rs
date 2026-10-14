//! `freerouting-gui` desktop app.

fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native("Freerouting", options, Box::new(|_cc| Ok(Box::new(App))))
}

struct App;

impl eframe::App for App {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Freerouting");
            ui.label("No board loaded. The board viewer is not implemented yet.");
        });
    }
}
