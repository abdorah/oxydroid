//! oxydroid demo: ordinary egui `TextEdit`s on Android, through `oxydroid::egui::Ime`.
//! - Sign-in form: username (Enter = Next → password), password (Enter = Done).
//! - Chat: Enter = Send; the field is cleared by the app (an app write, Q1).
//! - Back logs you out (Android 13+); dark mode follows the system; insets pad the top.
//! - Self-tests: the Q1 race and the per-keystroke snapshot cost.
#![cfg(target_os = "android")]

use egui::{Key, TextEdit};
use oxydroid::{EnterAction, Event, FieldKind, egui::Ime};

#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("oxydemo"),
    );
    if let Err(e) = oxydroid::init(app.vm_as_ptr(), app.activity_as_ptr()) {
        log::error!("oxydroid::init failed: {e:?}");
    }
    let options = eframe::NativeOptions {
        android_app: Some(app),
        ..Default::default()
    };
    eframe::run_native(
        "oxydemo",
        options,
        Box::new(|cc| {
            add_system_fonts(&cc.egui_ctx);
            Ok(Box::new(Demo::new(Ime::new(&cc.egui_ctx))))
        }),
    )
    .unwrap();
}

struct RaceTest {
    seq: u64,
    dropped_before: u64,
    saw_race_text: bool,
    frames: u32,
}

struct Demo {
    ime: Ime,
    user: String,
    pass: String,
    signed_in: Option<String>,
    focus_pass: bool,
    draft: String,
    messages: Vec<String>,
    results: Vec<String>,
    race: Option<RaceTest>,
    edge_to_edge: bool,
}

impl Demo {
    fn new(ime: Ime) -> Self {
        Self {
            ime,
            user: String::new(),
            pass: String::new(),
            signed_in: None,
            focus_pass: false,
            draft: String::new(),
            messages: vec!["Try: é, عربي, 😀, swipe, voice, autocorrect".into()],
            results: Vec::new(),
            race: None,
            edge_to_edge: false,
        }
    }

    fn result(&mut self, s: String) {
        log::info!("{s}");
        self.results.push(s);
    }

    fn sign_in(&mut self) {
        if !self.user.is_empty() {
            self.signed_in = Some(self.user.clone());
            self.pass.clear();
            oxydroid::set_back_handled(true); // Back = sign out (Android 13+)
        }
    }

    /// Q1 end to end on the device: a keyboard edit is already queued when the app writes.
    /// The keyboard's snapshot ("RACE") must be dropped; only the app's write may arrive.
    fn start_race_test(&mut self) {
        let dropped_before = oxydroid::dropped_snapshots();
        oxydroid::debug_keyboard_commit("RACE"); // queued on the UI thread first...
        let seq = oxydroid::set_state("", 0..0); // ...then the app write, stamped after it
        self.race = Some(RaceTest {
            seq,
            dropped_before,
            saw_race_text: false,
            frames: 0,
        });
    }

    fn check_race_test(&mut self, events: &[Event]) {
        let Some(t) = &mut self.race else { return };
        t.frames += 1;
        let mut echoed = false;
        for ev in events {
            if let Event::Text(s) = ev {
                t.saw_race_text |= s.text.contains("RACE");
                echoed |= s.last_app_seq >= t.seq;
            }
        }
        if echoed || t.frames > 120 {
            let dropped = oxydroid::dropped_snapshots() - t.dropped_before;
            let pass = echoed && !t.saw_race_text && dropped >= 1;
            let msg = format!(
                "race test: {} (stale dropped: {dropped}, RACE leaked: {}, echo received: {echoed})",
                if pass { "PASS" } else { "FAIL" },
                t.saw_race_text
            );
            self.race = None;
            self.result(msg);
        }
    }
}

impl eframe::App for Demo {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.ime.begin_frame(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let events = self.ime.take_events();
        for ev in &events {
            match ev {
                Event::Back => {
                    self.signed_in = None;
                    oxydroid::set_back_handled(false);
                    self.result("back → signed out".into());
                }
                Event::Config(c) => ctx.set_visuals(if c.night_mode {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                }),
                _ => {}
            }
        }
        self.check_race_test(&events);
        for m in oxydroid::debug_take_messages() {
            self.result(format!("latency: {m}"));
        }

        let ppp = ctx.pixels_per_point();
        let top = self.ime.insets.top as f32 / ppp;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(top + 4.0);
            let wide = ui.available_width();

            match self.signed_in.clone() {
                None => {
                    ui.heading("Sign in");
                    let user = self.ime.text_edit(
                        ui,
                        &mut self.user,
                        FieldKind::Text,
                        EnterAction::Next,
                        |t| {
                            TextEdit::singleline(t)
                                .hint_text("Username")
                                .desired_width(wide)
                        },
                    );
                    if user.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        self.focus_pass = true;
                    }
                    let pass = self.ime.text_edit(
                        ui,
                        &mut self.pass,
                        FieldKind::Password,
                        EnterAction::Done,
                        |t| {
                            TextEdit::singleline(t)
                                .password(true)
                                .hint_text("Password")
                                .desired_width(wide)
                        },
                    );
                    if std::mem::take(&mut self.focus_pass) {
                        pass.response.request_focus();
                    }
                    let enter =
                        pass.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if ui.button("Sign in").clicked() || enter {
                        self.sign_in();
                    }
                }
                Some(name) => {
                    ui.heading(format!("Chat, {name}  (Back signs out)"));
                    // Enter = Send without losing focus, so the keyboard stays open.
                    let draft = self.ime.text_edit(
                        ui,
                        &mut self.draft,
                        FieldKind::Text,
                        EnterAction::Send,
                        |t| {
                            TextEdit::singleline(t)
                                .hint_text("Message")
                                .return_key(None)
                                .desired_width(wide)
                        },
                    );
                    let send =
                        draft.response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if (ui.button("Send").clicked() || send) && !self.draft.trim().is_empty() {
                        self.messages.push(std::mem::take(&mut self.draft)); // app write: clear
                    }
                    for m in self.messages.iter().rev().take(6) {
                        ui.label(egui::RichText::new(m).size(18.0));
                    }
                }
            }

            ui.separator();
            ui.collapsing("Settings & self-tests", |ui| {
                if ui
                    .checkbox(&mut self.edge_to_edge, "Edge-to-edge")
                    .changed()
                {
                    oxydroid::set_edge_to_edge(self.edge_to_edge);
                }
                let i = self.ime.insets;
                ui.label(format!(
                    "insets px: top {} bottom {} | keyboard {}",
                    i.top, i.bottom, i.ime_bottom
                ));
                if let Some(c) = self.ime.config {
                    ui.label(format!(
                        "dark {} | font scale {:.2} | {} dpi",
                        c.night_mode, c.font_scale, c.density_dpi
                    ));
                }
                ui.label("Type in any field once first (the tests need a keyboard session).");
                ui.horizontal(|ui| {
                    if ui.button("Race test").clicked() && self.race.is_none() {
                        self.start_race_test();
                    }
                    if ui.button("Latency").clicked() {
                        for n in [1_000, 10_000, 100_000] {
                            oxydroid::debug_time_report(n);
                        }
                        oxydroid::set_state("", 0..0); // drops the measurement snapshots (Q1)
                    }
                });
                for r in &self.results {
                    ui.monospace(r);
                }
            });
        });

        self.ime.end_frame(&ctx);
    }
}

/// egui's built-in fonts have no Arabic and few emoji: borrow the phone's.
fn add_system_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, path) in [
        ("sys-arabic", "/system/fonts/NotoNaskhArabicUI-Regular.ttf"),
        ("sys-emoji", "/system/fonts/NotoColorEmoji.ttf"),
    ] {
        match std::fs::read(path) {
            Ok(bytes) => {
                fonts.font_data.insert(
                    name.into(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts.families.entry(family).or_default().push(name.into());
                }
            }
            Err(e) => log::warn!("font {path}: {e}"),
        }
    }
    ctx.set_fonts(fonts);
}
