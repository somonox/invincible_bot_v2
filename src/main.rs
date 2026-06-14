pub mod engine {
    pub mod header;
    pub mod piece;
    pub mod board;
    pub mod movegen;
    pub mod state;
}

pub mod rl {
    pub mod features;
    pub mod agent;
}

pub mod gui {
    pub mod app;
    pub mod widgets;
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 750.0])
            .with_title("Antigravity 4-Wide RL Bot")
            .with_min_inner_size([1000.0, 650.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Antigravity 4-Wide RL Bot",
        native_options,
        Box::new(|cc| Box::new(gui::app::TetrisApp::new(cc))),
    )
}
