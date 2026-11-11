//! JNI glue: load the embedded dex, bind the native callbacks, call the Java side.
//! Techniques adapted from accesskit_android's inject.rs (notes/01).
//!
//! One `Platform` per process (a NativeActivity app has one activity), so global state is enough
//! and Java never holds Rust pointers: there's nothing to dangle.

use crate::{Config, EnterAction, Error, Event, FieldKind, Insets, TextState, offsets};
use jni::{
    JNIEnv, JavaVM, NativeMethod,
    objects::{GlobalRef, JClass, JObject, JString, JValue},
    sys::{jboolean, jfloat, jint, jlong},
};
use std::{
    collections::VecDeque,
    ffi::c_void,
    ops::Range,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

const CLASS: &str = "dev.kotbyte.oxydroid.Platform";
static DEX: &[u8] = include_bytes!("../classes.dex");

struct Glue {
    vm: JavaVM,
    class: GlobalRef,
}

static GLUE: OnceLock<Glue> = OnceLock::new();
static EVENTS: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());
static WAKER: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);
static INSETS: Mutex<Insets> = Mutex::new(Insets {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
    ime_bottom: 0,
});
static CONFIG: Mutex<Option<Config>> = Mutex::new(None);
/// Sequence number of the latest app write (Q1).
static APP_SEQ: AtomicU64 = AtomicU64::new(0);
/// Stale snapshots dropped by `poll` so far (for the race self-test).
static DROPPED: AtomicU64 = AtomicU64::new(0);
static DEBUG: Mutex<Vec<String>> = Mutex::new(Vec::new());

impl From<jni::errors::Error> for Error {
    fn from(e: jni::errors::Error) -> Self {
        Error::Jni(e)
    }
}

/// Load the helper and attach to the activity. Call at the top of `android_main`.
/// Safe to call again after the activity was recreated: it re-attaches to the new one.
/// `vm` and `activity` come from `AndroidApp::vm_as_ptr()` / `activity_as_ptr()`.
pub fn init(vm: *mut c_void, activity: *mut c_void) -> Result<(), Error> {
    // Borrowed global ref owned by android-activity: never delete it.
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    if let Some(glue) = GLUE.get() {
        let mut env = glue.vm.attach_current_thread_permanently()?;
        let class: &JClass = glue.class.as_obj().into();
        checked(&mut env, |env| {
            env.call_static_method(
                class,
                "install",
                "(Landroid/app/Activity;)V",
                &[(&activity).into()],
            )
            .map(|_| ())
        })?;
        return Ok(());
    }

    let vm = unsafe { JavaVM::from_raw(vm.cast()) }?;
    let mut env = vm.attach_current_thread_permanently()?;
    let class = checked(&mut env, |env| {
        // Null parent = boot class loader, which has every android.* class we use.
        let buf = unsafe { env.new_direct_byte_buffer(DEX.as_ptr() as *mut u8, DEX.len()) }?;
        let loader = env.new_object(
            "dalvik/system/InMemoryDexClassLoader",
            "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
            &[(&buf).into(), (&JObject::null()).into()],
        )?;
        let name = env.new_string(CLASS)?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[(&name).into()],
            )?
            .l()?;
        let class = JClass::from(class);
        // Runtime-loaded class: JNI can't find our symbols by name, so bind them explicitly.
        let natives = [
            (
                "nativeOnState",
                "(Ljava/lang/String;IIIIJ)V",
                on_state as *mut c_void,
            ),
            ("nativeOnAction", "(I)V", on_action as *mut c_void),
            ("nativeOnKey", "(II)V", on_key as *mut c_void),
            ("nativeOnInsets", "(IIIII)V", on_insets as *mut c_void),
            ("nativeOnBack", "()V", on_back as *mut c_void),
            ("nativeOnConfig", "(ZFI)V", on_config as *mut c_void),
            (
                "nativeOnDebug",
                "(Ljava/lang/String;)V",
                on_debug as *mut c_void,
            ),
        ];
        let natives: Vec<NativeMethod> = natives
            .into_iter()
            .map(|(name, sig, fn_ptr)| NativeMethod {
                name: name.into(),
                sig: sig.into(),
                fn_ptr,
            })
            .collect();
        env.register_native_methods(&class, &natives)?;
        env.call_static_method(
            &class,
            "install",
            "(Landroid/app/Activity;)V",
            &[(&activity).into()],
        )?;
        env.new_global_ref(class)
    })?;

    // A concurrent first init would have to race on two threads; keep the first one.
    let _ = GLUE.set(Glue { vm, class });
    log::info!("oxydroid: loaded {} bytes of dex", DEX.len());
    Ok(())
}

/// Called (on the Android UI thread) whenever an event is queued. Use it to request a repaint.
pub fn set_waker(f: impl Fn() + Send + 'static) {
    *WAKER.lock().unwrap() = Some(Box::new(f));
}

/// Drain queued events. Drops keyboard snapshots that predate the latest app write (Q1).
pub fn poll() -> Vec<Event> {
    let latest = APP_SEQ.load(Ordering::SeqCst);
    EVENTS
        .lock()
        .unwrap()
        .drain(..)
        .filter(|ev| match ev {
            Event::Text(s) if s.last_app_seq < latest => {
                DROPPED.fetch_add(1, Ordering::Relaxed);
                log::debug!(
                    "oxydroid: dropped stale snapshot (seq {} < {latest})",
                    s.last_app_seq
                );
                false
            }
            _ => true,
        })
        .collect()
}

/// The latest insets (also delivered as [`Event::Insets`]).
pub fn insets() -> Insets {
    *INSETS.lock().unwrap()
}

/// The latest configuration, once known (also delivered as [`Event::Config`]).
pub fn config() -> Option<Config> {
    *CONFIG.lock().unwrap()
}

// ---- Keyboard ----

/// Show the keyboard for a field of this kind. Call when a text field gains focus. Changing
/// `kind`/`action` restarts the keyboard session (new layout).
pub fn show(kind: FieldKind, action: EnterAction) {
    call(
        "show",
        "(II)V",
        &[JValue::Int(kind as jint), JValue::Int(action as jint)],
    );
}

pub fn hide() {
    call("hide", "()V", &[]);
}

/// App write: replaces text and selection (Q1: always wins). Restarts the keyboard session.
/// Returns the sequence number stamped on it. Same as [`set_text`] with a forward selection.
pub fn set_state(text: &str, selection: Range<usize>) -> u64 {
    set_text(text, selection.start, selection.end)
}

/// App write: replaces the text and sets the selection, keeping its direction (`head` may be
/// before `anchor`). One keyboard restart. Returns the sequence number.
pub fn set_text(text: &str, anchor: usize, head: usize) -> u64 {
    let seq = APP_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    // Android's Selection.setSelection(start, stop) keeps start as the anchor even if start > stop.
    let (s, e) = (utf16(text, anchor), utf16(text, head));
    with_env(|env| {
        let jtext = env.new_string(text)?;
        env.call_static_method(
            class(),
            "setState",
            "(Ljava/lang/String;IIJ)V",
            &[
                (&jtext).into(),
                JValue::Int(s),
                JValue::Int(e),
                JValue::Long(seq as jlong),
            ],
        )
        .map(|_| ())
    });
    seq
}

/// App moved the caret or selection (tap, drag, its own arrow handling) in `text`, the app's
/// current text. `head` may be before `anchor`. No keyboard restart. Returns the sequence number.
pub fn set_selection(text: &str, anchor: usize, head: usize) -> u64 {
    let seq = APP_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let args = [
        JValue::Int(utf16(text, anchor)),
        JValue::Int(utf16(text, head)),
        JValue::Long(seq as jlong),
    ];
    call("setSelection", "(IIJ)V", &args);
    seq
}

/// Arrow keys are *visual* on Android (in Arabic, Right moves backwards). Only the app's layout
/// knows where "visually right" is. `true`: arrows arrive as [`Event::Key`] for the app to handle.
/// `false` (default): the mirror moves one character logically, which is fine for left-to-right text.
pub fn set_arrows_to_app(to_app: bool) {
    call(
        "setArrowsToApp",
        "(Z)V",
        &[JValue::Bool(to_app as jboolean)],
    );
}

/// Show the floating Cut/Copy/Paste/Select-all toolbar above `rect` (window pixels, usually the
/// selection's bounds). Actions apply to the text and arrive as [`Event::Text`].
pub fn show_toolbar(left: i32, top: i32, right: i32, bottom: i32) {
    let args = [
        JValue::Int(left),
        JValue::Int(top),
        JValue::Int(right),
        JValue::Int(bottom),
    ];
    call("showToolbar", "(IIII)V", &args);
}

pub fn hide_toolbar() {
    call("hideToolbar", "()V", &[]);
}

// ---- Window ----

/// Draw behind transparent status and navigation bars. Android 16 forces this when targeting
/// API 36; use [`insets`] to keep content clear of the bars.
pub fn set_edge_to_edge(on: bool) {
    call("setEdgeToEdge", "(Z)V", &[JValue::Bool(on as jboolean)]);
}

/// `true`: back gestures arrive as [`Event::Back`] and the app decides (close a dialog, go up
/// a screen). `false`: the system handles back (usually finishes the app, with the predictive
/// animation). Android 13+; older versions deliver back as a key event through the input queue.
pub fn set_back_handled(handled: bool) {
    call(
        "setBackHandled",
        "(Z)V",
        &[JValue::Bool(handled as jboolean)],
    );
}

// ---- helpers ----

fn utf16(text: &str, byte: usize) -> jint {
    offsets::byte_to_utf16(text, byte) as jint
}

fn class() -> &'static JClass<'static> {
    GLUE.get()
        .expect("oxydroid::init not called")
        .class
        .as_obj()
        .into()
}

fn call(name: &str, sig: &str, args: &[JValue]) {
    with_env(|env| env.call_static_method(class(), name, sig, args).map(|_| ()));
}

fn with_env(f: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<()>) {
    let Some(glue) = GLUE.get() else {
        log::warn!("oxydroid: not initialized");
        return;
    };
    match glue.vm.attach_current_thread_permanently() {
        Ok(mut env) => {
            if let Err(e) = checked(&mut env, f) {
                log::error!("oxydroid: JNI call failed: {e:?}");
            }
        }
        Err(e) => log::error!("oxydroid: attach failed: {e:?}"),
    }
}

/// A pending Java exception poisons every later JNI call: log it and clear it.
fn checked<T>(
    env: &mut JNIEnv,
    f: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<T>,
) -> jni::errors::Result<T> {
    let r = f(env);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    r
}

fn push(ev: Event) {
    EVENTS.lock().unwrap().push_back(ev);
    if let Some(w) = WAKER.lock().unwrap().as_ref() {
        w();
    }
}

/// A panic must never unwind into Java (undefined behaviour): catch and log it.
fn guard(name: &str, f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        log::error!("oxydroid: panic in {name} callback (caught)");
    }
}

// ---- callbacks from Java (UI thread) ----

#[allow(clippy::too_many_arguments)]
extern "system" fn on_state(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
    sel_start: jint,
    sel_end: jint,
    comp_start: jint,
    comp_end: jint,
    last_app_seq: jlong,
) {
    guard("state", || {
        let text: String = match env.get_string(&text) {
            Ok(s) => s.into(),
            Err(e) => return log::error!("oxydroid: bad string: {e:?}"),
        };
        let b = |i: jint| offsets::utf16_to_byte(&text, i.max(0) as usize);
        // Android's selection start is the anchor and end is the head; keep the direction.
        let (anchor, head) = (b(sel_start), b(sel_end));
        let composing = (comp_start >= 0 && comp_end >= 0).then(|| {
            let (a, z) = (b(comp_start), b(comp_end));
            a.min(z)..a.max(z)
        });
        push(Event::Text(TextState {
            text,
            anchor,
            head,
            composing,
            last_app_seq: last_app_seq as u64,
        }));
    });
}

extern "system" fn on_action(_env: JNIEnv, _class: JClass, action: jint) {
    guard("action", || push(Event::Action(action)));
}

extern "system" fn on_key(_env: JNIEnv, _class: JClass, code: jint, meta: jint) {
    const META_SHIFT_ON: jint = 1;
    guard("key", || {
        push(Event::Key {
            code,
            shift: meta & META_SHIFT_ON != 0,
        })
    });
}

extern "system" fn on_insets(
    _env: JNIEnv,
    _class: JClass,
    l: jint,
    t: jint,
    r: jint,
    b: jint,
    ime: jint,
) {
    guard("insets", || {
        let i = Insets {
            left: l,
            top: t,
            right: r,
            bottom: b,
            ime_bottom: ime,
        };
        let changed = std::mem::replace(&mut *INSETS.lock().unwrap(), i) != i;
        if changed {
            push(Event::Insets(i));
        }
    });
}

extern "system" fn on_back(_env: JNIEnv, _class: JClass) {
    guard("back", || push(Event::Back));
}

extern "system" fn on_config(
    _env: JNIEnv,
    _class: JClass,
    night: jboolean,
    font_scale: jfloat,
    dpi: jint,
) {
    guard("config", || {
        let c = Config {
            night_mode: night != 0,
            font_scale,
            density_dpi: dpi,
        };
        *CONFIG.lock().unwrap() = Some(c);
        push(Event::Config(c));
    });
}

extern "system" fn on_debug(mut env: JNIEnv, _class: JClass, message: JString) {
    guard("debug", || {
        if let Ok(s) = env.get_string(&message) {
            DEBUG.lock().unwrap().push(s.into());
        }
    });
}

// ---- Self-test hooks (used by the demo; not part of the stable API) ----

/// Simulate the keyboard committing `text`, through the real InputConnection path.
#[doc(hidden)]
pub fn debug_keyboard_commit(text: &str) {
    with_env(|env| {
        let t = env.new_string(text)?;
        env.call_static_method(
            class(),
            "debugKeyboardCommit",
            "(Ljava/lang/String;)V",
            &[(&t).into()],
        )
        .map(|_| ())
    });
}

/// Measure the per-keystroke snapshot cost for a text of `chars` characters. The result arrives
/// in [`debug_take_messages`]. Follow with an app write (e.g. `set_state`) to drop the snapshots it makes.
#[doc(hidden)]
pub fn debug_time_report(chars: i32) {
    call("debugTimeReport", "(I)V", &[JValue::Int(chars)]);
}

#[doc(hidden)]
pub fn debug_take_messages() -> Vec<String> {
    std::mem::take(&mut *DEBUG.lock().unwrap())
}

/// How many stale keyboard snapshots `poll` has dropped (Q1 filter).
#[doc(hidden)]
pub fn dropped_snapshots() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

/// A string extra of the launch intent (`adb shell am start --es key value`, or a deep link).
/// `activity` is `AndroidApp::activity_as_ptr()`.
pub fn intent_extra(activity: *mut c_void, key: &str) -> Option<String> {
    let glue = GLUE.get()?;
    let mut env = glue.vm.attach_current_thread_permanently().ok()?;
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let class: &JClass = glue.class.as_obj().into();
    checked(&mut env, |env| {
        let k = env.new_string(key)?;
        let v = env
            .call_static_method(
                class,
                "intentExtra",
                "(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;",
                &[(&activity).into(), (&k).into()],
            )?
            .l()?;
        if v.is_null() {
            return Ok(None);
        }
        Ok(Some(env.get_string(&JString::from(v))?.into()))
    })
    .ok()
    .flatten()
}

/// Simulate one keyboard call on the InputConnection, as a keyboard app makes it:
/// `op` = `commit`/`compose` (text), `finish`, `region` (a, b), `delete` (a = code points before),
/// `select` (a, b in UTF-16 units), `menu` (a = `android.R.id.*`), `key` (a = keycode).
#[doc(hidden)]
pub fn debug_keyboard(op: &str, text: &str, a: i32, b: i32) {
    with_env(|env| {
        let (o, t) = (env.new_string(op)?, env.new_string(text)?);
        env.call_static_method(
            class(),
            "debugKeyboard",
            "(Ljava/lang/String;Ljava/lang/String;II)V",
            &[(&o).into(), (&t).into(), JValue::Int(a), JValue::Int(b)],
        )
        .map(|_| ())
    });
}
