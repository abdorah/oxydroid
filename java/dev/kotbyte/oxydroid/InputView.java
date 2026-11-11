// oxydroid: keyboard (IME) support for Rust apps on NativeActivity.
// Licensed under MIT OR Apache-2.0.
//
// An invisible 1x1 View that owns the keyboard session. It keeps a mirror of the text
// (so the keyboard's synchronous questions are answered instantly on the UI thread) and
// sends one snapshot to Rust after every edit. All decisions live in Rust.
// Only touched on the main thread (via Platform).

package dev.kotbyte.oxydroid;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.graphics.Rect;
import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.view.ActionMode;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

final class InputView extends View {
    private final Editable mEditable = Editable.Factory.getInstance().newEditable("");
    private final InputMethodManager mImm;
    private long mLastAppSeq = 0;   // Q1: echo of the last app write we applied
    private int mInputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
    private int mImeOptions = EditorInfo.IME_ACTION_DONE;
    boolean mArrowsToApp = false;
    private ActionMode mToolbar;
    private final Rect mToolbarRect = new Rect();

    InputView(Context ctx) {
        super(ctx);
        setFocusable(true);
        setFocusableInTouchMode(true);
        mImm = (InputMethodManager) ctx.getSystemService(Context.INPUT_METHOD_SERVICE);
    }

    // ---- From Platform (main thread) ----

    void show(int kind, int action) {
        int type;
        switch (kind) {
            case 1: type = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD; break;
            case 2: type = InputType.TYPE_CLASS_NUMBER; break;
            case 3: type = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS; break;
            case 4: type = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                    | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT; break;
            default: type = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
        }
        int[] actions = { EditorInfo.IME_ACTION_NONE, EditorInfo.IME_ACTION_DONE, EditorInfo.IME_ACTION_GO,
                EditorInfo.IME_ACTION_SEND, EditorInfo.IME_ACTION_NEXT, EditorInfo.IME_ACTION_SEARCH };
        int options = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_FLAG_NO_EXTRACT_UI
                | actions[Math.max(0, Math.min(action, actions.length - 1))];
        if (kind == 1) options |= EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING;
        if (kind == 4) options |= EditorInfo.IME_FLAG_NO_ENTER_ACTION;   // Enter = newline

        boolean changedKind = type != mInputType || options != mImeOptions;
        mInputType = type;
        mImeOptions = options;
        requestFocus();
        if (changedKind) mImm.restartInput(this);   // new EditorInfo: a different keyboard layout
        mImm.showSoftInput(this, 0);
    }

    void hide() {
        hideToolbar();
        mImm.hideSoftInputFromWindow(getWindowToken(), 0);
    }

    /** Q1: app writes always win. Replace the mirror, restart the keyboard session, echo. */
    void setState(String text, int selStart, int selEnd, long appSeq) {
        mLastAppSeq = appSeq;
        mEditable.replace(0, mEditable.length(), text);
        BaseInputConnection.removeComposingSpans(mEditable);
        int len = mEditable.length();
        Selection.setSelection(mEditable, clamp(selStart, len), clamp(selEnd, len));
        mImm.restartInput(this);
        report();
    }

    /** App moved the caret/selection (e.g. a tap). Like TextView: no restart, just tell the IME,
     *  which then decides itself whether to finish its composing word. */
    void setSelection(int selStart, int selEnd, long appSeq) {
        mLastAppSeq = appSeq;
        int len = mEditable.length();
        Selection.setSelection(mEditable, clamp(selStart, len), clamp(selEnd, len));
        report();
    }

    private static int clamp(int x, int len) { return Math.max(0, Math.min(x, len)); }

    // ---- Floating toolbar (Cut / Copy / Paste / Select all) ----

    void showToolbar(Rect windowRect) {
        mToolbarRect.set(windowRect);
        if (mToolbar != null) {
            mToolbar.invalidateContentRect();
            return;
        }
        mToolbar = startActionMode(new ActionMode.Callback2() {
            @Override public boolean onCreateActionMode(ActionMode m, Menu menu) {
                menu.add(Menu.NONE, android.R.id.cut, 0, android.R.string.cut);
                menu.add(Menu.NONE, android.R.id.copy, 1, android.R.string.copy);
                menu.add(Menu.NONE, android.R.id.paste, 2, android.R.string.paste);
                menu.add(Menu.NONE, android.R.id.selectAll, 3, android.R.string.selectAll);
                return true;
            }
            @Override public boolean onPrepareActionMode(ActionMode m, Menu menu) { return false; }
            @Override public boolean onActionItemClicked(ActionMode m, MenuItem item) {
                contextAction(item.getItemId(), true);
                m.finish();
                return true;
            }
            @Override public void onDestroyActionMode(ActionMode m) { mToolbar = null; }
            @Override public void onGetContentRect(ActionMode m, View v, Rect out) {
                // The app gives window coordinates; ActionMode wants this view's coordinates.
                int[] loc = new int[2];
                getLocationInWindow(loc);
                out.set(mToolbarRect);
                out.offset(-loc[0], -loc[1]);
            }
        }, ActionMode.TYPE_FLOATING);
    }

    void hideToolbar() {
        if (mToolbar != null) mToolbar.finish();
    }

    /** Shared by the keyboard's context menu and our toolbar. */
    private boolean contextAction(int id, boolean fromToolbar) {
        int a = Selection.getSelectionStart(mEditable), b = Selection.getSelectionEnd(mEditable);
        int s = Math.min(a, b), en = Math.max(a, b);
        ClipboardManager cm = (ClipboardManager) getContext().getSystemService(Context.CLIPBOARD_SERVICE);
        boolean textChanged = false;
        switch (id) {
            case android.R.id.selectAll:
                Selection.selectAll(mEditable);
                break;
            case android.R.id.copy:
            case android.R.id.cut:
                if (s == en) return true;
                cm.setPrimaryClip(ClipData.newPlainText(null, mEditable.subSequence(s, en).toString()));
                if (id == android.R.id.cut) {
                    mEditable.delete(s, en);
                    textChanged = true;
                }
                break;
            case android.R.id.paste: {
                ClipData clip = cm.getPrimaryClip();
                if (clip == null || clip.getItemCount() == 0) return true;
                CharSequence t = clip.getItemAt(0).coerceToText(getContext());
                BaseInputConnection.removeComposingSpans(mEditable);
                mEditable.replace(s, en, t);
                Selection.setSelection(mEditable, s + t.length());
                textChanged = true;
                break;
            }
            default:
                return false;
        }
        // The keyboard asked for it, so it knows; for our own toolbar, resync the keyboard.
        if (fromToolbar && textChanged) mImm.restartInput(this);
        report();
        return true;
    }

    // ---- Keyboard side ----

    @Override public boolean onCheckIsTextEditor() { return true; }

    @Override public InputConnection onCreateInputConnection(EditorInfo out) {
        out.inputType = mInputType;
        out.imeOptions = mImeOptions;
        out.initialSelStart = Selection.getSelectionStart(mEditable);
        out.initialSelEnd = Selection.getSelectionEnd(mEditable);
        mConn = new Connection();
        return mConn;
    }

    // ---- Test hooks (used by the demo's self-tests; not part of the public API) ----

    private Connection mConn;

    /** Simulate the keyboard committing text, through the same path a real keyboard uses. */
    void debugKeyboardCommit(String text) {
        debugKeyboard("commit", text, 0, 0);
    }

    /** Simulate one keyboard call on the InputConnection, exactly as a keyboard app makes it.
     *  op: commit(text) | compose(text) | finish | delete(a = code points before) |
     *      select(a, b) | menu(a = android.R.id.*) | key(a = keycode). */
    void debugKeyboard(String op, String text, int a, int b) {
        if (mConn == null) mConn = new Connection();
        switch (op) {
            case "commit": mConn.commitText(text, 1); break;
            case "compose": mConn.setComposingText(text, 1); break;
            case "finish": mConn.finishComposingText(); break;
            case "delete": mConn.deleteSurroundingTextInCodePoints(a, 0); break;
            case "select": mConn.setSelection(a, b); break;
            case "menu": mConn.performContextMenuAction(a); break;
            case "key": mConn.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, a)); break;
            default: break;
        }
    }

    /** Average cost of one per-keystroke snapshot (toString + JNI + Rust) for a text of n chars.
     *  The snapshots it produces carry the current app seq: follow with an app write to drop them. */
    String debugTimeReport(int n) {
        char[] filler = new char[n];
        java.util.Arrays.fill(filler, 'a');
        String saved = mEditable.toString();
        mEditable.replace(0, mEditable.length(), new String(filler));
        final int rounds = 20;
        long t0 = System.nanoTime();
        for (int i = 0; i < rounds; i++) report();
        long avgUs = (System.nanoTime() - t0) / rounds / 1000;
        mEditable.replace(0, mEditable.length(), saved);
        return n + " chars: " + avgUs + " us per snapshot";
    }

    /** Send one snapshot to Rust, and keep the keyboard's model of the cursor in sync. */
    private void report() {
        int ss = Selection.getSelectionStart(mEditable);
        int se = Selection.getSelectionEnd(mEditable);
        int cs = BaseInputConnection.getComposingSpanStart(mEditable);
        int ce = BaseInputConnection.getComposingSpanEnd(mEditable);
        mImm.updateSelection(this, ss, se, cs, ce);
        Platform.nativeOnState(mEditable.toString(), ss, se, cs, ce, mLastAppSeq);
    }

    private final class Connection extends BaseInputConnection {
        private int mBatch = 0;
        private boolean mShiftHeld = false;

        Connection() { super(InputView.this, true); }   // full editor: edits go to getEditable()

        @Override public Editable getEditable() { return mEditable; }

        // Batch edits: report once at the end, never a half-applied state.
        @Override public boolean beginBatchEdit() { mBatch++; return true; }
        @Override public boolean endBatchEdit() {
            if (mBatch > 0 && --mBatch == 0) report();
            return mBatch > 0;
        }
        private boolean changed(boolean result) {
            hideToolbar();                       // typing dismisses the toolbar, like EditText
            if (mBatch == 0) report();
            return result;
        }

        // Commands: BaseInputConnection applies them to the mirror; we report afterwards.
        @Override public boolean commitText(CharSequence t, int pos) { return changed(super.commitText(t, pos)); }
        @Override public boolean setComposingText(CharSequence t, int pos) { return changed(super.setComposingText(t, pos)); }
        @Override public boolean setComposingRegion(int s, int e) { return changed(super.setComposingRegion(s, e)); }
        @Override public boolean finishComposingText() { return changed(super.finishComposingText()); }
        @Override public boolean deleteSurroundingText(int b, int a) { return changed(super.deleteSurroundingText(b, a)); }
        @Override public boolean deleteSurroundingTextInCodePoints(int b, int a) { return changed(super.deleteSurroundingTextInCodePoints(b, a)); }
        @Override public boolean setSelection(int s, int e) { return changed(super.setSelection(s, e)); }

        @Override public boolean performEditorAction(int action) { Platform.nativeOnAction(action); return true; }

        /** Keyboard's Select all / Cut / Copy / Paste. BaseInputConnection does nothing here. */
        @Override public boolean performContextMenuAction(int id) { return contextAction(id, false); }

        /** Q3: apply it if it means something to the mirror; otherwise forward it to the app. */
        @Override public boolean sendKeyEvent(KeyEvent e) {
            int code = e.getKeyCode();
            // Samsung's "Select" toggle sends SHIFT down/up around the arrow presses.
            if (code == KeyEvent.KEYCODE_SHIFT_LEFT || code == KeyEvent.KEYCODE_SHIFT_RIGHT) {
                mShiftHeld = e.getAction() == KeyEvent.ACTION_DOWN;
                return true;
            }
            if (e.getAction() != KeyEvent.ACTION_DOWN) return true;
            boolean shift = mShiftHeld || e.isShiftPressed();
            int s = Selection.getSelectionStart(mEditable);
            int en = Selection.getSelectionEnd(mEditable);
            boolean arrow = code == KeyEvent.KEYCODE_DPAD_LEFT || code == KeyEvent.KEYCODE_DPAD_RIGHT;
            if (arrow && mArrowsToApp) {
                // Arrows are VISUAL; only the app's (bidi-aware) layout can move correctly.
                Platform.nativeOnKey(code, shift ? KeyEvent.META_SHIFT_ON : 0);
                return true;
            }
            if (code == KeyEvent.KEYCODE_DEL) {
                if (s != en) {
                    mEditable.delete(Math.min(s, en), Math.max(s, en));
                    return changed(true);
                }
                if (s > 0) return deleteSurroundingTextInCodePoints(1, 0);
            } else if (arrow) {
                // Logical fallback: one code point. Wrong in right-to-left text (see notes/05).
                String t = mEditable.toString();
                boolean left = code == KeyEvent.KEYCODE_DPAD_LEFT;
                if (left ? en > 0 : en < t.length()) {
                    int target = t.offsetByCodePoints(en, left ? -1 : 1);
                    // Android keeps the anchor in selection *start*; Shift moves only the end.
                    if (shift) Selection.extendSelection(mEditable, target);
                    else Selection.setSelection(mEditable, target);
                    return changed(true);
                }
            } else {
                int ch = e.getUnicodeChar();
                if (ch != 0 && !Character.isISOControl(ch)) {
                    return commitText(new String(Character.toChars(ch)), 1);
                }
            }
            Platform.nativeOnKey(code, shift ? KeyEvent.META_SHIFT_ON : 0);   // the app decides
            return true;
        }
    }
}
