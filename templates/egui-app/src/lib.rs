//! Starter: an egui app on Android with a working soft keyboard.
//!
//! Ordinary `egui::TextEdit`s; `oxydroid::egui::Ime` connects them to the Android keyboard
//! (accents, any script, emoji, suggestions, swipe, voice), with native selection handles and
//! the Cut/Copy/Paste toolbar. Dark mode and the status bar follow the system.
#![cfg(target_os = "android")]

use egui::{Key, TextEdit};
use oxydroid::{EnterAction, Event, FieldKind, egui::Ime};

#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    if let Err(e) = oxydroid::init(app.vm_as_ptr(), app.activity_as_ptr()) {
        log::error!("oxydroid::init failed: {e:?}");
    }
    let options = eframe::NativeOptions {
        android_app: Some(app),
        ..Default::default()
    };
    eframe::run_native(
        "app",
        options,
        Box::new(|cc| {
            add_system_fonts(&cc.egui_ctx);
            Ok(Box::new(App {
                ime: Ime::new(&cc.egui_ctx),
                note: String::new(),
                notes: Vec::new(),
            }))
        }),
    )
    .unwrap();
}

struct App {
    ime: Ime,
    note: String,
    notes: Vec<String>,
}

impl eframe::App for App {
    // 1. Keyboard → egui, before every frame.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.ime.begin_frame(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for ev in self.ime.take_events() {
            if let Event::Config(c) = ev {
                ctx.set_visuals(if c.night_mode {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
            }
        }
        // Keep the UI below the status bar: Android 16 draws apps edge-to-edge.
        let top = self.ime.insets.top as f32 / ctx.pixels_per_point();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(top + 8.0);
            ui.heading("Notes");
            // 2. Any TextEdit, through the adapter. Enter = Send, without losing focus.
            let width = ui.available_width();
            let field = self.ime.text_edit(
                ui,
                &mut self.note,
                FieldKind::Text,
                EnterAction::Send,
                |t| {
                    TextEdit::singleline(t)
                        .hint_text("Write a note")
                        .return_key(None)
                        .desired_width(width)
                },
            );
            let enter = field.response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.button("Add").clicked() || enter) && !self.note.trim().is_empty() {
                self.notes.push(std::mem::take(&mut self.note)); // the app clears the field
            }
            for n in self.notes.iter().rev() {
                ui.label(egui::RichText::new(n).size(18.0));
            }
        });

        // 3. Last, after every text field.
        self.ime.end_frame(&ctx);
    }
}

/// egui's built-in fonts have no Arabic and few emoji: borrow the phone's own as fallbacks.
fn add_system_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, path) in [
        ("sys-arabic", "/system/fonts/NotoNaskhArabicUI-Regular.ttf"),
        ("sys-emoji", "/system/fonts/NotoColorEmoji.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                name.into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push(name.into());
            }
        }
    }
    ctx.set_fonts(fonts);
}
