#![windows_subsystem = "windows"]

// Temporary native UI driver. No scientific inputs or product state are assigned here.
use std::{collections::VecDeque, path::PathBuf};
use eframe::{egui, App};

struct NativeDriver {
    app: sekai::TemplateApp,
    command_path: PathBuf,
    events: VecDeque<egui::Event>,
}

impl App for NativeDriver {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
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
                } else if let Some(coords) = command.trim().strip_prefix("drag ") {
                    let parsed: Vec<f32> = coords.split_whitespace()
                        .filter_map(|part| part.parse().ok()).collect();
                    if let [start_x, start_y, end_x, end_y] = parsed.as_slice() {
                        let start = egui::pos2(*start_x, *start_y);
                        let end = egui::pos2(*end_x, *end_y);
                        self.events.push_back(egui::Event::PointerMoved(start));
                        self.events.push_back(egui::Event::PointerButton {
                            pos: start,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        });
                        for step in 1..=8 {
                            self.events.push_back(egui::Event::PointerMoved(
                                start + (end - start) * (step as f32 / 8.0),
                            ));
                        }
                        self.events.push_back(egui::Event::PointerButton {
                            pos: end,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        });
                    }
                } else if let Some(coords) = command.trim().strip_prefix("wheel ") {
                    let parsed: Vec<f32> = coords.split_whitespace()
                        .filter_map(|part| part.parse().ok()).collect();
                    if let [x, y, delta_y] = parsed.as_slice() {
                        self.events.push_back(egui::Event::PointerMoved(egui::pos2(*x, *y)));
                        self.events.push_back(egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, *delta_y),
                            modifiers: egui::Modifiers::NONE,
                        });
                    }
                } else if command.trim() == "ctrl-alt-f" {
                    for pressed in [true, false] {
                        self.events.push_back(egui::Event::Key {
                            key: egui::Key::F,
                            physical_key: Some(egui::Key::F),
                            pressed,
                            repeat: false,
                            modifiers: if pressed {
                                egui::Modifiers { ctrl: true, alt: true, command: true,
                                    ..egui::Modifiers::NONE }
                            } else {
                                egui::Modifiers::NONE
                            },
                        });
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
            if let egui::Event::Key { modifiers, .. } = &event {
                raw.modifiers = *modifiers;
            }
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
