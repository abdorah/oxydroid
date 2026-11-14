//! Optional self-test (safe to delete, with the `mod selftest` line in lib.rs).
//! Launch with:  adb shell am start -S -n <package>/android.app.NativeActivity --es oxytest 1
//! It focuses the field, then drives the REAL InputConnection the way a keyboard app does, and
//! logs `oxytest: PASS|FAIL …` (logcat tag RustStdoutStderr / your app's tag).

use crate::Field;
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::EditableText,
};

#[derive(Resource)]
struct SelfTest {
    step: u32,
    next_at: f32,
}

pub fn plugin(app: &mut App) {
    #[cfg(target_os = "android")]
    {
        let wanted = bevy::android::ANDROID_APP
            .get()
            .and_then(|a| oxydroid_bevy::oxydroid::intent_extra(a.activity_as_ptr(), "oxytest"))
            .is_some();
        if wanted {
            app.insert_resource(SelfTest {
                step: 0,
                next_at: 3.0,
            })
            .add_systems(Update, run);
        }
    }
    let _ = app;
}

fn run(
    time: Res<Time>,
    mut test: ResMut<SelfTest>,
    mut focus: ResMut<InputFocus>,
    fields: Query<(Entity, &EditableText), With<Field>>,
) {
    if time.elapsed_secs() < test.next_at {
        return;
    }
    let Ok((field, text)) = fields.single() else {
        return;
    };
    let kb = oxydroid_bevy::oxydroid::debug_keyboard;
    let value = text.value().to_string();
    let sel = text.editor().raw_selection();
    let sel = (sel.anchor().index(), sel.focus().index());
    let check = |what: &str, ok: bool| {
        info!(
            "oxytest: {} {what}: {value:?}",
            if ok { "PASS" } else { "FAIL" }
        );
    };
    match test.step {
        0 => focus.set(field, FocusCause::Pressed), // like a tap: Bevy enables IME, the plugin opens the keyboard
        1 => kb("commit", "héllo ", 0, 0),
        2 => {
            check("commit with an accent", value == "héllo ");
            kb("compose", "wo", 0, 0); // the keyboard composes, underlined
        }
        3 => {
            // The user picks the suggestion: a keyboard re-marks its word and commits over it,
            // back to back. (The live system keyboard may already have finished our simulated
            // composition, since it didn't start it: re-marking makes the step deterministic.)
            kb("region", "", 6, 8);
            kb("commit", "world", 0, 0);
        }
        4 => {
            check("commit replaces the composition", value == "héllo world");
            kb("delete", "", 1, 0); // backspace
        }
        5 => {
            check("delete one character", value == "héllo worl");
            kb("key", "", 21, 0); // left arrow: Bevy moves its caret, the plugin tells the keyboard
        }
        6 => kb("commit", "!", 0, 0), // must land where Bevy's caret now is
        7 => {
            check(
                "arrow key moved Bevy's caret, keyboard followed",
                value == "héllo wor!l",
            );
            kb("select", "", 6, 9); // the keyboard selects "wor" (UTF-16 offsets)
        }
        8 => {
            check("keyboard selection reaches Bevy", sel == (7, 10));
            kb("key", "", 67, 0); // delete key over the selection
        }
        9 => {
            check("delete removes the whole selection", value == "héllo !l");
            // Samsung's Select mode: Shift held, then arrows.
            kb("key", "", 59, 0);
            kb("key", "", 21, 0);
            kb("key", "", 21, 0);
        }
        10 => {
            check("Shift+arrows extend Bevy's selection", sel == (7, 5));
            kb("key", "", 67, 0);
        }
        11 => {
            check("delete after Shift+arrows", value == "héll!l");
            info!("oxytest: shot bevy-text-input");
        }
        _ => {
            info!("oxytest: DONE");
            test.next_at = f32::MAX;
            return;
        }
    }
    test.step += 1;
    test.next_at = time.elapsed_secs() + 1.5;
}
