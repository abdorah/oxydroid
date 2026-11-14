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
/// InputConnection.CURSOR_UPDATE_*
const CURSOR_UPDATE_IMMEDIATE: i32 = 1;
const CURSOR_UPDATE_MONITOR: i32 = 2;
const STEP_SECONDS: f64 = 1.0;
/// screencap over Wi-Fi takes ~1 s: hold the state that long after a shot cue.
const SHOT_HOLD_SECONDS: f64 = 2.5;

pub(crate) struct AutoTest {
    step: usize,
    next_at: f64,
    passed: u32,
    failed: u32,
    done: bool,
    /// Where the end-handle drag started (screen px), for the handle test.
    drag: Option<(i32, i32)>,
}

impl AutoTest {
    pub(crate) fn new() -> Self {
        Self {
            step: 0,
            next_at: 2.0,
            passed: 0,
            failed: 0,
            done: false,
            drag: None,
        }
    }
}

/// Ask the test runner (autotest.sh) to run `adb shell input ARGS`, once per `n`.
fn input(n: u32, args: &str) {
    log::info!("oxytest: input#{n} {args}");
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
                // Android sends the keyboard's slide animation frame by frame only to apps that
                // draw edge-to-edge (forced from Android 15 at target 35+). Turn it on below that,
                // so the animation check means the same thing on every version.
                if oxydroid::debug_api_level() < 35 {
                    oxydroid::set_edge_to_edge(true);
                }
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
                // What Gboard asks for: the caret's screen position, and the whole text.
                kb(
                    "cursorupdates",
                    "",
                    CURSOR_UPDATE_IMMEDIATE | CURSOR_UPDATE_MONITOR,
                    0,
                );
                kb("extract", "", 1, 0);
                Then::Act
            }
            16 => {
                let extracted = self
                    .results
                    .iter()
                    .rev()
                    .find_map(|r| r.strip_prefix("latency: extracted=").map(str::to_owned));
                self.check(
                    "getExtractedText returns the text and selection",
                    extracted.as_deref() == Some("app wrote sel=9..9"),
                    format!("{extracted:?}"),
                );
                self.focus_pass = true; // move to the password field
                Then::Act
            }
            17 => {
                kb("commit", "s3cret", 0, 0);
                Then::Act
            }
            18 => {
                let ok = self.pass == "s3cret";
                self.check(
                    "password field gets text",
                    ok,
                    format!("{} chars", self.pass.chars().count()),
                );
                Then::Shot("17-password")
            }
            19 => {
                self.start_race_test();
                Then::Act
            }
            20 => {
                for n in [1_000, 10_000, 100_000] {
                    oxydroid::debug_time_report(n);
                }
                oxydroid::set_state(&self.pass.clone(), 0..0); // drop the measurement snapshots
                Then::Act
            }
            21 => {
                // Back (Android 13+): close the keyboard first, or the keyboard eats the Back.
                ctx.memory_mut(|m| m.surrender_focus(m.focused().unwrap_or(egui::Id::NULL)));
                oxydroid::hide();
                oxydroid::set_back_handled(true);
                Then::Act
            }
            22 => {
                self.back_seen = false;
                input(1, "keyevent KEYCODE_BACK"); // a real key press, from the test runner
                Then::Act
            }
            23 => {
                let api = oxydroid::debug_api_level();
                if api >= 33 {
                    let seen = self.back_seen;
                    self.check(
                        "Back reaches the app (OnBackInvokedCallback)",
                        seen,
                        format!("API {api}"),
                    );
                } else {
                    self.result(format!("oxytest: SKIP Back on API {api} (needs 13+)"));
                }
                // The demo's Back handler already gave Back back to the system: don't press it again.
                Then::Act
            }
            24 => {
                // Native handles: refocus the field and let the keyboard select "app".
                if let Some(id) = self.user_id {
                    ctx.memory_mut(|m| m.request_focus(id));
                }
                Then::Act
            }
            25 => {
                kb("select", "", 0, 3);
                Then::Act
            }
            26 => {
                // Grab the end handle by its body: it hangs below its tip, and its tip sits at a
                // quarter of its width, so aim a little right of and below the tip. Real touches.
                if let oxydroid::Handles::Selection { end: (x, y), .. } = self.ime.handles() {
                    let (gx, gy) = (x as i32 + 15, y as i32 + 30);
                    self.autotest.as_mut().unwrap().drag = Some((gx, gy));
                    input(2, &format!("motionevent DOWN {gx} {gy}"));
                } else {
                    let h = self.ime.handles();
                    self.check(
                        "selection shows two native handles",
                        false,
                        format!("{h:?}"),
                    );
                }
                Then::Act
            }
            27 => {
                if let Some((gx, gy)) = self.autotest.as_ref().unwrap().drag {
                    input(3, &format!("motionevent MOVE {} {gy}", gx + 300));
                }
                Then::Act
            }
            28 => Then::Shot("28-handle-drag-magnifier"),
            29 => {
                if let Some((gx, gy)) = self.autotest.as_ref().unwrap().drag {
                    input(4, &format!("motionevent UP {} {gy}", gx + 300));
                }
                Then::Act
            }
            30 => {
                let range = self
                    .user_id
                    .and_then(|id| TextEditState::load(ctx, id))
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| (r.secondary.index.0, r.primary.index.0));
                let ok = range.is_some_and(|(a, h)| a == 0 && h > 3);
                self.check(
                    "dragging the native end handle extends the selection",
                    ok,
                    format!("{range:?}"),
                );
                Then::Shot("30-after-handle-drag")
            }
            _ => {
                let race_pass = self
                    .results
                    .iter()
                    .any(|r| r.starts_with("race test: PASS"));
                self.check("race test (Q1)", race_pass, "see race line".into());

                // The keyboard opened and closed several times: with the animation callback
                // (API 30+) the app sees the intermediate heights, not just 0 and the final one.
                let mut heights = self.ime_heights.clone();
                heights.sort_unstable();
                heights.dedup();
                let api = oxydroid::debug_api_level();
                if api >= 30 {
                    self.check(
                        "keyboard slide arrives frame by frame",
                        heights.len() >= 6,
                        format!(
                            "{} distinct heights, max {:?} px",
                            heights.len(),
                            heights.last()
                        ),
                    );
                }
                let cfg = self.ime.config;
                self.check(
                    "theme info: accent color + animations",
                    cfg.is_some_and(|c| c.accent_color != 0),
                    format!(
                        "{:?}",
                        cfg.map(|c| (format!("#{:08X}", c.accent_color), c.animations_enabled))
                    ),
                );
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
