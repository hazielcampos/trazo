//! UI panels around the canvas.

pub mod audio;
pub mod brushes;
pub mod camera;
pub mod home;
pub mod dialogs;
pub mod export;
pub mod layers;
pub mod phone;
pub mod selection_bar;
pub mod timeline;
pub mod toolbar;
pub mod top_bar;
pub mod video_import;

use crate::app::TrazoApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideTab {
    Layers,
    Brushes,
    Color,
}

/// Right-hand panel with tabs (layers / brushes / color).
pub fn side_panel(app: &mut TrazoApp, ui: &mut egui::Ui) {
    egui::Panel::right("side_panel").resizable(true).default_size(300.0).min_size(200.0).show(ui, |ui| {
        timeline::commit_selection_on_click(app, ui);
        ui.add_space(4.0);
        match app.ui.side_tab {
            SideTab::Layers => layers::show(app, ui),
            SideTab::Brushes => brushes::show(app, ui),
            SideTab::Color => toolbar::color_panel(app, ui),
        }
    });
}
