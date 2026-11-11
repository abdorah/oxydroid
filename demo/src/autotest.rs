//! Scripted on-device test, started with:
//!   adb shell am start -S -n dev.kotbyte.oxydroid.demo/android.app.NativeActivity --es oxytest 1
//! Every step drives the REAL InputConnection exactly as a keyboard app calls it
//! (`oxydroid::debug_keyboard`), then checks the egui field on the next step.
//! Log lines (tag oxydemo): `oxytest: PASS|FAIL …`, `oxytest: shot NAME` (screenshot cue; the
//! test then pauses so the capture shows that state), `oxytest: DONE passed=N failed=M`.

use super::Demo;
use egui::text_edit::TextEditState;

/// android.R.id values used by keyboards' context menus.
const SELECT_ALL: i32 = 0x0102_001f;
const CUT: i32 = 0x0102_0020;
const PASTE: i32 = 0x0102_0022;
const KEYCODE_DEL: i32 = 67;
const STEP_SECONDS: f64 = 1.0;
/// screencap over Wi-Fi takes ~1 s: hold the state that long after a shot cue.
const SHOT_HOLD_SECONDS: f64 = 2.5;

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

/// What a step does after checking: an action (keyboard call) or a screenshot pause.
enum Then {
    Act,
    Shot(&'static str),
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
        let kb = oxydroid::debug_keyboard;
        let then = match t.step {
            0 => {
                if let Some(id) = self.user_id {
                    ctx.memory_mut(|m| m.request_focus(id)); // like a tap on the field
                }
                Then::Act
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
                Then::Act
            }
            2 => {
                self.expect_user("compose (é)", "hé");
                Then::Shot("02-composing-underlined")
            }
            3 => {
                // The user picks the suggestion: the keyboard re-marks its word and commits over
                // it, back to back (the live Samsung keyboard may have finished our fake
                // composition in the meantime, as it doesn't know about it).
                kb("region", "", 0, 2);
                kb("commit", "héllo", 0, 0);
                Then::Act
            }
            4 => {
                self.expect_user("commit replaces the composing word", "héllo");
                kb("commit", " عربي 😀", 0, 0);
                Then::Act
            }
            5 => {
                self.expect_user("Arabic + emoji", "héllo عربي 😀");
                Then::Shot("05-arabic-emoji")
            }
            6 => {
                kb("delete", "", 1, 0); // backspace: one code point (the emoji)
                Then::Act
            }
            7 => {
                self.expect_user(
                    "delete one code point (emoji = 2 UTF-16 units)",
                    "héllo عربي ",
                );
                kb("select", "", 0, 5); // keyboard selects "héllo" (UTF-16 offsets)
                Then::Act
            }
            8 => {
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
                Then::Shot("08-selection-handles-toolbar")
            }
            9 => {
                kb("commit", "HELLO", 0, 0); // typing replaces the selection
                Then::Act
            }
            10 => {
                self.expect_user("typing replaces the selection", "HELLO عربي ");
                kb("menu", "", SELECT_ALL, 0);
                kb("menu", "", CUT, 0);
                Then::Act
            }
            11 => {
                self.expect_user("select all + cut", "");
                kb("menu", "", PASTE, 0);
                Then::Act
            }
            12 => {
                self.expect_user("paste (system clipboard)", "HELLO عربي ");
                self.user = "app wrote".into(); // app write: the adapter must resync the keyboard
                Then::Act
            }
            13 => {
                kb("commit", "!", 0, 0); // lands after "app wrote" only if Java got the app write
                Then::Act
            }
            14 => {
                self.expect_user("app write reached the keyboard's mirror", "app wrote!");
                kb("key", "", KEYCODE_DEL, 0);
                Then::Act
            }
            15 => {
                self.expect_user("DEL key event", "app wrote");
                self.focus_pass = true; // move to the password field
                Then::Act
            }
            16 => {
                kb("commit", "s3cret", 0, 0);
                Then::Act
            }
            17 => {
                let ok = self.pass == "s3cret";
                self.check(
                    "password field gets text",
                    ok,
                    format!("{} chars", self.pass.chars().count()),
                );
                Then::Shot("17-password")
            }
            18 => {
                self.start_race_test();
                Then::Act
            }
            19 => {
                for n in [1_000, 10_000, 100_000] {
                    oxydroid::debug_time_report(n);
                }
                oxydroid::set_state(&self.pass.clone(), 0..0); // drop the measurement snapshots
                Then::Act
            }
            _ => {
                let race_pass = self
                    .results
                    .iter()
                    .any(|r| r.starts_with("race test: PASS"));
                self.check("race test (Q1)", race_pass, "see race line".into());
                let t = self.autotest.as_mut().unwrap();
                t.done = true;
                let (p, f) = (t.passed, t.failed);
                self.result(format!("oxytest: DONE passed={p} failed={f}"));
                log::info!("oxytest: shot 99-results");
                return;
            }
        };
        let t = self.autotest.as_mut().unwrap();
        t.step += 1;
        t.next_at = now
            + match then {
                Then::Act => STEP_SECONDS,
                Then::Shot(name) => {
                    log::info!("oxytest: shot {name}");
                    SHOT_HOLD_SECONDS
                }
            };
    }
}
