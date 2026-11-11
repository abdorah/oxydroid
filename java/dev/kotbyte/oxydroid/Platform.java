// oxydroid: platform services for Rust apps on Android NativeActivity.
// Licensed under MIT OR Apache-2.0.
//
// The only class Rust talks to. Every public method may be called from any thread; work is
// posted to the main looper in FIFO order. All native callbacks live here (RegisterNatives).
// Framework API only (no AndroidX), so it runs from a prebuilt in-memory dex.

package dev.kotbyte.oxydroid;

import android.app.Activity;
import android.content.ComponentCallbacks;
import android.content.res.Configuration;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.Rect;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowInsets;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

public final class Platform {
    private static final Handler sMain = new Handler(Looper.getMainLooper());
    static Activity sActivity;
    static InputView sInput;
    private static Object sBackCallback;          // OnBackInvokedCallback, API 33+ only
    private static boolean sConfigRegistered;
    private static int sStatusColor, sNavColor;   // to restore after edge-to-edge
    private static boolean sColorsSaved;

    // ---- Rust callbacks ----
    static native void nativeOnState(String text, int selStart, int selEnd,
                                     int compStart, int compEnd, long lastAppSeq);
    static native void nativeOnAction(int editorAction);
    static native void nativeOnKey(int keyCode, int metaState);
    static native void nativeOnInsets(int left, int top, int right, int bottom, int imeBottom);
    static native void nativeOnBack();
    static native void nativeOnConfig(boolean night, float fontScale, int densityDpi);
    static native void nativeOnDebug(String message);

    private Platform() {}

    private static void post(Runnable r) { sMain.post(r); }

    // ---- Setup ----

    /** Idempotent per activity: after a recreation, Rust calls this again with the new one. */
    public static void install(final Activity activity) {
        post(new Runnable() {
            @Override public void run() {
                if (sActivity == activity) return;
                sActivity = activity;
                sBackCallback = null;      // belonged to the old activity
                sColorsSaved = false;
                sInput = new InputView(activity);
                // Covers the WHOLE window (on the decor view, not the content area, which starts
                // below the status/title bars) so its coordinates match the native surface: the
                // floating toolbar only shows over a rect inside its originating view. It can't
                // steal touches (NativeActivity takes the input queue) and draws nothing (native
                // code owns the surface).
                ViewGroup decor = (ViewGroup) activity.getWindow().getDecorView();
                decor.addView(sInput, new ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
                installInsets(activity);
                if (!sConfigRegistered) {
                    sConfigRegistered = true;
                    activity.getApplicationContext().registerComponentCallbacks(new ComponentCallbacks() {
                        @Override public void onConfigurationChanged(Configuration c) { reportConfig(c); }
                        @SuppressWarnings("deprecation")   // required by the interface
                        @Override public void onLowMemory() {}
                    });
                }
                reportConfig(activity.getResources().getConfiguration());
            }
        });
    }

    // ---- Keyboard ----

    /** kind: 0 text, 1 password, 2 number, 3 email, 4 multiline. action: 0 none, 1 done,
     *  2 go, 3 send, 4 next, 5 search. */
    public static void show(final int kind, final int action) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.show(kind, action); }
        });
    }

    public static void hide() {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.hide(); }
        });
    }

    public static void setState(final String text, final int selStart, final int selEnd, final long appSeq) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.setState(text, selStart, selEnd, appSeq); }
        });
    }

    public static void setSelection(final int selStart, final int selEnd, final long appSeq) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.setSelection(selStart, selEnd, appSeq); }
        });
    }

    /** true: arrow keys go to the app (visual, bidi-aware movement). false: logical fallback. */
    public static void setArrowsToApp(final boolean toApp) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.mArrowsToApp = toApp; }
        });
    }

    /** Floating Cut/Copy/Paste/Select-all toolbar near this rect (window pixels). */
    public static void showToolbar(final int left, final int top, final int right, final int bottom) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.showToolbar(new Rect(left, top, right, bottom)); }
        });
    }

    public static void hideToolbar() {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.hideToolbar(); }
        });
    }

    // ---- Insets ----

    private static void installInsets(Activity a) {
        View decor = a.getWindow().getDecorView();
        decor.setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
            @Override public WindowInsets onApplyWindowInsets(View v, WindowInsets in) {
                reportInsets(in);
                return v.onApplyWindowInsets(in);   // keep the default behaviour
            }
        });
        decor.requestApplyInsets();
    }

    @SuppressWarnings("deprecation")
    private static void reportInsets(WindowInsets in) {
        if (Build.VERSION.SDK_INT >= 30) {
            Insets s = in.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            Insets ime = in.getInsets(WindowInsets.Type.ime());
            nativeOnInsets(s.left, s.top, s.right, s.bottom, ime.bottom);
        } else {
            int imeBottom = Math.max(0, in.getSystemWindowInsetBottom() - in.getStableInsetBottom());
            nativeOnInsets(in.getStableInsetLeft(), in.getStableInsetTop(),
                    in.getStableInsetRight(), in.getStableInsetBottom(), imeBottom);
        }
    }

    /** Draw behind transparent system bars (forced anyway when targeting API 36 on Android 16). */
    @SuppressWarnings("deprecation")
    public static void setEdgeToEdge(final boolean on) {
        post(new Runnable() {
            @Override public void run() {
                if (sActivity == null) return;
                Window w = sActivity.getWindow();
                if (!sColorsSaved) {
                    sStatusColor = w.getStatusBarColor();
                    sNavColor = w.getNavigationBarColor();
                    sColorsSaved = true;
                }
                if (Build.VERSION.SDK_INT >= 30) {
                    w.setDecorFitsSystemWindows(!on);
                } else {
                    int f = View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION;
                    w.getDecorView().setSystemUiVisibility(on ? f : 0);
                }
                w.setStatusBarColor(on ? Color.TRANSPARENT : sStatusColor);
                w.setNavigationBarColor(on ? Color.TRANSPARENT : sNavColor);
                w.getDecorView().requestApplyInsets();
            }
        });
    }

    // ---- Back ----

    /** true: back gestures go to the app (Event::Back). false: the system handles them (finish,
     *  predictive back). Android 13+ only; older versions deliver back as a key event. */
    public static void setBackHandled(final boolean handled) {
        post(new Runnable() {
            @Override public void run() {
                if (sActivity == null || Build.VERSION.SDK_INT < 33) return;
                if (handled && sBackCallback == null) {
                    sBackCallback = Back33.register(sActivity);
                } else if (!handled && sBackCallback != null) {
                    Back33.unregister(sActivity, sBackCallback);
                    sBackCallback = null;
                }
            }
        });
    }

    /** Only loaded on API 33+, so older devices never verify these classes. */
    private static final class Back33 {
        static Object register(Activity a) {
            OnBackInvokedCallback cb = new OnBackInvokedCallback() {
                @Override public void onBackInvoked() { nativeOnBack(); }
            };
            a.getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT, cb);
            return cb;
        }

        static void unregister(Activity a, Object cb) {
            a.getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback((OnBackInvokedCallback) cb);
        }
    }

    // ---- Launch intent ----

    /** A string extra of the launch intent (e.g. `adb shell am start --es key value`, deep links).
     *  Reads the activity given to install(); synchronous, any thread. */
    public static String intentExtra(Activity activity, String key) {
        android.content.Intent i = activity.getIntent();
        return i == null ? null : i.getStringExtra(key);
    }

    // ---- Test hooks (demo self-tests; not public API) ----

    public static void debugKeyboardCommit(final String text) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.debugKeyboardCommit(text); }
        });
    }

    public static void debugKeyboard(final String op, final String text, final int a, final int b) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) sInput.debugKeyboard(op, text, a, b); }
        });
    }

    public static void debugTimeReport(final int chars) {
        post(new Runnable() {
            @Override public void run() { if (sInput != null) nativeOnDebug(sInput.debugTimeReport(chars)); }
        });
    }

    // ---- Configuration (dark mode, font scale, density) ----

    static void reportConfig(Configuration c) {
        boolean night = (c.uiMode & Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES;
        nativeOnConfig(night, c.fontScale, c.densityDpi);
    }
}
