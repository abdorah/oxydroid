//! Scripted on-device test, started with:
//!   adb shell am start -S -n dev.kotbyte.oxydroid.demo/android.app.NativeActivity --es oxytest 1
//! Every step drives the REAL InputConnection exactly as a keyboard app calls it
//! (`oxydroid::debug_keyboard`), then checks the egui field one step later.
//! Log lines (tag oxydemo): `oxytest: PASS|FAIL …`, `oxytest: shot NAME` (screenshot cue),
//! `oxytest: DONE passed=N failed=M`.

use super::Demo;
use egui::text_edit::TextEditState;

/// android.R.id values used by keyboards' context menus.
const SELECT_ALL: i32 = 0x0102_001f;
const CUT: i32 = 0x0102_0020;
const PASTE: i32 = 0x0102_0022;
const KEYCODE_DEL: i32 = 67;
const STEP_SECONDS: f64 = 1.2;

pub(crate) struct AutoTest {
    step: usize,
    next_at: f64,
    passed: u32,
    failed: u32,
    done: bool,
}

impl AutoTest {
    pub(crate) fn new() -> Self {
        Self {
            step: 0,
            next_at: 2.0,
            passed: 0,
            failed: 0,
            done: false,
        }
    }
}

fn shot(name: &str) {
    log::info!("oxytest: shot {name}");
}

impl Demo {
    fn check(&mut self, what: &str, ok: bool, detail: String) {
        let t = self.autotest.as_mut().unwrap();
        if ok {
            t.passed += 1;
        } else {
            t.failed += 1;
        }
        let line = format!(
            "oxytest: {} {what}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        );
        self.result(line);
    }

    fn expect_user(&mut self, what: &str, expected: &str) {
        let ok = self.user == expected;
        self.check(what, ok, format!("{:?} (expected {expected:?})", self.user));
    }

    /// Advance one step when its time has come. Call every frame while the test runs.
    pub(crate) fn run_autotest(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.autotest else { return };
        if t.done {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let now = ctx.input(|i| i.time);
        if now < t.next_at {
            return;
        }
        let step = t.step;
        let kb = oxydroid::debug_keyboard;
        match step {
            0 => {
                shot("00-start");
                if let Some(id) = self.user_id {
                    ctx.memory_mut(|m| m.request_focus(id)); // like a tap on the field
                }
            }
            1 => {
                let focused = self
                    .user_id
                    .is_some_and(|id| ctx.memory(|m| m.has_focus(id)));
                self.check(
                    "focus opens a keyboard session",
                    focused,
                    format!("focused={focused}"),
                );
                kb("compose", "hé", 0, 0); // keyboard shows "hé" underlined
            }
            2 => {
                self.expect_user("compose (é)", "hé");
                shot("02-composing");
                kb("commit", "héllo", 0, 0); // user picks the suggestion
            }
            3 => {
                self.expect_user("commit replaces the composing word", "héllo");
                kb("commit", " عربي 😀", 0, 0);
            }
            4 => {
                self.expect_user("Arabic + emoji", "héllo عربي 😀");
                shot("04-arabic-emoji");
                kb("delete", "", 1, 0); // backspace: one code point (the emoji)
            }
            5 => {
                self.expect_user(
                    "delete one code point (emoji = 2 UTF-16 units)",
                    "héllo عربي ",
                );
                kb("select", "", 0, 5); // keyboard selects "héllo" (UTF-16 offsets)
            }
            6 => {
                let range = self
                    .user_id
                    .and_then(|id| TextEditState::load(ctx, id))
                    .and_then(|s| s.cursor.char_range());
                let got = range.map(|r| (r.secondary.index.0, r.primary.index.0));
                self.check(
                    "keyboard selection reaches egui",
                    got == Some((0, 5)),
                    format!("{got:?}"),
                );
                shot("06-selection-handles-toolbar");
                kb("commit", "HELLO", 0, 0); // typing replaces the selection
            }
            7 => {
                self.expect_user("typing replaces the selection", "HELLO عربي ");
                kb("menu", "", SELECT_ALL, 0);
                kb("menu", "", CUT, 0);
            }
            8 => {
                self.expect_user("select all + cut", "");
                kb("menu", "", PASTE, 0);
            }
            9 => {
                self.expect_user("paste (system clipboard)", "HELLO عربي ");
                self.user = "app wrote".into(); // app write: the adapter must resync the keyboard
            }
            10 => {
                kb("commit", "!", 0, 0); // lands after "app wrote" only if Java got the app write
            }
            11 => {
                self.expect_user("app write reached the keyboard's mirror", "app wrote!");
                kb("key", "", KEYCODE_DEL, 0);
            }
            12 => {
                self.expect_user("DEL key event", "app wrote");
                self.focus_pass = true; // move to the password field
            }
            13 => {
                kb("commit", "s3cret", 0, 0);
            }
            14 => {
                let ok = self.pass == "s3cret";
                self.check(
                    "password field gets text",
                    ok,
                    format!("{} chars", self.pass.chars().count()),
                );
                shot("14-password");
                self.start_race_test();
            }
            15 => {
                // The race test reports itself (PASS/FAIL line) within a few frames.
                for n in [1_000, 10_000, 100_000] {
                    oxydroid::debug_time_report(n);
                }
                oxydroid::set_state(&self.pass.clone(), 0..0); // drop the measurement snapshots
            }
            _ => {
                let race_pass = self
                    .results
                    .iter()
                    .any(|r| r.starts_with("race test: PASS"));
                self.check("race test (Q1)", race_pass, "see race line".into());
                shot("16-results");
                let t = self.autotest.as_mut().unwrap();
                t.done = true;
                let (p, f) = (t.passed, t.failed);
                self.result(format!("oxytest: DONE passed={p} failed={f}"));
                return;
            }
        }
        let t = self.autotest.as_mut().unwrap();
        t.step += 1;
        t.next_at = now + STEP_SECONDS;
    }
}
