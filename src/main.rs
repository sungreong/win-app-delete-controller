#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fonts;
mod installed_app;
mod scanner;
mod settings;
mod ui;
mod uninstaller;

use eframe::egui;
use ui::DeleteControllerApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([860.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Windows App Delete Controller",
        options,
        Box::new(|cc| {
            fonts::configure_korean_fonts(&cc.egui_ctx);
            ui::configure_visuals(&cc.egui_ctx);
            Ok(Box::new(DeleteControllerApp::new()))
        }),
    )
}
