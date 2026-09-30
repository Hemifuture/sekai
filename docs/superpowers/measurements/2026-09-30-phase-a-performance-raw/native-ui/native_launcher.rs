#![windows_subsystem = "windows"]

// Temporary native UI driver. No scientific inputs or product state are assigned here.
use std::{collections::VecDeque, path::PathBuf, time::Duration};
use eframe::{egui, App};

struct NativeDriver {
    app: sekai::TemplateApp,
    command_path: PathBuf,
    events: VecDeque<egui::Event>,
}

impl App for NativeDriver {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
        if self.events.is_empty() {
            if let Ok(command) = std::fs::read_to_string(&self.command_path) {
                let _ = std::fs::remove_file(&self.command_path);
                if let Some(coords) = command.trim().strip_prefix("click ") {
                    let parsed: Vec<f32> = coords.split_whitespace()
                        .filter_map(|part| part.parse().ok()).collect();
                    if let [x, y] = parsed.as_slice() {
                        let pos = egui::pos2(*x, *y);
                        self.events.push_back(egui::Event::PointerMoved(pos));
                        for pressed in [true, false] {
                            self.events.push_back(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                    }
                } else if let Some(text) = command.strip_prefix("text ") {
                    self.events.push_back(egui::Event::Text(text.trim_end().to_owned()));
                } else if command.trim() == "close" {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Some(event) = self.events.pop_front() {
            // Scope keyboard/pointer focus to egui's input; do not activate the OS window.
            raw.focused = true;
            raw.events.push(event);
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    let scratch = PathBuf::from(std::env::args_os().nth(1).expect("scratch path required"));
    assert!(scratch.is_absolute(), "scratch must be absolute");
    let options = eframe::NativeOptions {
        persistence_path: Some(scratch.join("isolated-native-state.ron")),
        persist_window: false,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 900.0])
            .with_min_inner_size([800.0, 600.0])
            .with_active(false),
        ..Default::default()
    };
    eframe::run_native("Sekai - World Generator (isolated native capture)", options,
        Box::new(move |cc| Ok(Box::new(NativeDriver {
            app: sekai::TemplateApp::new(cc),
            command_path: scratch.join("ui-command.txt"),
            events: VecDeque::new(),
        }))))
}
