// oxydroid: keyboard (IME) support for Rust apps on NativeActivity.
// Licensed under MIT OR Apache-2.0.
//
// An invisible full-window View that owns the keyboard session. It keeps a mirror of the text
// (so the keyboard's synchronous questions are answered instantly on the UI thread) and
// sends one snapshot to Rust after every edit. All decisions live in Rust.
// Only touched on the main thread (via Platform).

package dev.kotbyte.oxydroid;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.graphics.Matrix;
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
import android.view.inputmethod.CursorAnchorInfo;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.ExtractedText;
import android.view.inputmethod.ExtractedTextRequest;
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
    // Caret geometry from the app (window px), for CursorAnchorInfo. NaN = unknown.
    private float mCaretX = Float.NaN;
    private float mCaretTop;
    private float mCaretBottom;
    private boolean mCaretRtl;
    private final int[] mScreenLocation = new int[2];
    private String mReportedText = "";   // text of the last snapshot sent to Rust

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
        setHandles(0, 0, 0, 0, 0, 0);
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

    // ---- Selection handles + magnifier ----

    private Handle mCaretHandle;
    private Handle mStartHandle;
    private Handle mEndHandle;
    private int mHandleColor;
    private int mLineHeight;
    private Object mMagnifier;   // android.widget.Magnifier, API 29+ only

    /** mode 0: none, 1: caret handle at (ax, ay), 2: selection handles at (ax, ay) and (bx, by).
     *  Positions are the tips, in window pixels: the caret's x at the bottom of its line. */
    void setHandles(int mode, int ax, int ay, int bx, int by, int lineHeight) {
        mLineHeight = lineHeight;
        if (mode == 1) {
            hideHandle(mStartHandle);
            hideHandle(mEndHandle);
            mCaretHandle = placeHandle(mCaretHandle, Handle.CARET, ax, ay);
        } else if (mode == 2) {
            hideHandle(mCaretHandle);
            mStartHandle = placeHandle(mStartHandle, Handle.START, ax, ay);
            mEndHandle = placeHandle(mEndHandle, Handle.END, bx, by);
        } else {
            hideHandle(mCaretHandle);
            hideHandle(mStartHandle);
            hideHandle(mEndHandle);
        }
        if (mToolbar != null) mToolbar.invalidateContentRect();   // it avoids the handles
        showMagnifier();   // the dragged handle snapped to a new caret position
    }

    void setHandleColor(int argb) {
        mHandleColor = argb;
        for (Handle h : new Handle[] { mCaretHandle, mStartHandle, mEndHandle }) {
            if (h != null) h.setColor(argb);
        }
    }

    private Handle placeHandle(Handle h, int id, int x, int y) {
        if (h == null) {
            h = new Handle(this, id);
            h.setColor(mHandleColor);
        }
        h.place(x, y);
        return h;
    }

    private static void hideHandle(Handle h) {
        if (h != null) h.hide();
    }

    private int visibleHandleHeight() {
        int max = 0;
        for (Handle h : new Handle[] { mCaretHandle, mStartHandle, mEndHandle }) {
            if (h != null && h.isShowing()) max = Math.max(max, h.getDrawable().getIntrinsicHeight());
        }
        return max;
    }

    /** While a handle is dragged: hide the toolbar and show the magnifier over the text. */
    void onHandleDrag(Handle h, float x, float y, int phase) {
        if (phase == Handle.PHASE_DOWN) hideToolbar();
        mDragged = phase == Handle.PHASE_UP ? null : h;
        mDragX = x;
        if (android.os.Build.VERSION.SDK_INT < 29) return;
        if (phase == Handle.PHASE_UP) {
            if (mMagnifier != null) Magnifier29.dismiss(mMagnifier);
            mMagnifier = null;   // rebuilt next drag, in case the density changed
            return;
        }
        if (mMagnifier == null) mMagnifier = Magnifier29.create(this);
        showMagnifier();
    }

    private Handle mDragged;
    private float mDragX;

    /** Follow the finger, but stay within half a line of the caret (where the app snapped the
     *  handle), so the magnifier shows text rather than the empty space past the end of a line.
     *  Centred on the text line, just above the tip. This view covers the window at (0, 0), so
     *  window coordinates are view coordinates. */
    private void showMagnifier() {
        if (mMagnifier == null || mDragged == null) return;
        float half = mLineHeight / 2f;
        float tipX = mDragged.tipX();
        float x = Math.max(tipX - half, Math.min(mDragX, tipX + half));
        Magnifier29.show(mMagnifier, x, mDragged.tipY() - half);
    }

    /** Only loaded on API 29+. NativeActivity draws into the window's surface, which a magnifier
     *  of this (window-covering) view can copy. */
    private static final class Magnifier29 {
        static Object create(View v) {
            return new android.widget.Magnifier.Builder(v).build();
        }

        static void show(Object m, float x, float y) {
            ((android.widget.Magnifier) m).show(x, y);
        }

        static void dismiss(Object m) {
            ((android.widget.Magnifier) m).dismiss();
        }
    }

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
                // (They match: this view covers the whole window, see Platform.install.)
                int[] loc = new int[2];
                getLocationInWindow(loc);
                out.set(mToolbarRect);
                // Include the handles hanging below the text, as Android's text fields do: the
                // toolbar is placed above this rect (or below it when there's no room), never
                // over the handles, so they stay grabbable.
                out.bottom += visibleHandleHeight();
                out.offset(-loc[0], -loc[1]);
            }
        }, ActionMode.TYPE_FLOATING);
        if (mToolbar == null) android.util.Log.w("oxydroid", "floating toolbar refused by the window");
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
     *  op: commit(text) | compose(text) | finish | region(a, b) | delete(a = code points before) |
     *      select(a, b) | menu(a = android.R.id.*) | key(a = keycode). */
    void debugKeyboard(String op, String text, int a, int b) {
        if (mConn == null) mConn = new Connection();
        switch (op) {
            case "commit": mConn.commitText(text, 1); break;
            case "compose": mConn.setComposingText(text, 1); break;
            case "finish": mConn.finishComposingText(); break;
            case "region": mConn.setComposingRegion(a, b); break;
            case "delete": mConn.deleteSurroundingTextInCodePoints(a, 0); break;
            case "select": mConn.setSelection(a, b); break;
            case "menu": mConn.performContextMenuAction(a); break;
            case "key": mConn.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, a)); break;
            case "cursorupdates": mConn.requestCursorUpdates(a); break;
            case "extract": {
                ExtractedTextRequest req = new ExtractedTextRequest();
                req.token = a;
                ExtractedText et = mConn.getExtractedText(req, b);
                Platform.nativeOnDebug(et == null ? "extracted=null"
                        : "extracted=" + et.text + " sel=" + et.selectionStart + ".." + et.selectionEnd);
                break;
            }
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

    /** Caret geometry in window pixels, from the app's own layout. Needed by keyboards that
     *  place UI next to the caret: Gboard's floating-cursor and handwriting, OEM prediction
     *  overlays. `hasCaret` false while nothing is being edited. */
    void setCaret(boolean hasCaret, float x, float top, float bottom, boolean rtl) {
        mCaretX = hasCaret ? x : Float.NaN;
        mCaretTop = top;
        mCaretBottom = bottom;
        mCaretRtl = rtl;
        if (mConn != null) {
            mConn.reportCursorAnchorInfoIfMonitoring();
        }
    }

    /** Send one snapshot to Rust, and keep the keyboard's model of the editor in sync. */
    private void report() {
        int ss = Selection.getSelectionStart(mEditable);
        int se = Selection.getSelectionEnd(mEditable);
        int cs = BaseInputConnection.getComposingSpanStart(mEditable);
        int ce = BaseInputConnection.getComposingSpanEnd(mEditable);
        mImm.updateSelection(this, ss, se, cs, ce);
        if (mConn != null) {
            mConn.reportExtractedTextIfMonitoring();
            mConn.reportCursorAnchorInfoIfMonitoring();
        }
        mReportedText = mEditable.toString();
        Platform.nativeOnState(mReportedText, ss, se, cs, ce, mLastAppSeq);
    }

    private final class Connection extends BaseInputConnection {
        private int mBatch = 0;
        private boolean mShiftHeld = false;
        private boolean mMonitorCursor = false;
        private CursorAnchorInfo.Builder mAnchorBuilder;
        private final Matrix mViewToScreen = new Matrix();
        private ExtractedTextRequest mExtractRequest;   // non-null = the IME is monitoring
        private final ExtractedText mExtracted = new ExtractedText();

        Connection() { super(InputView.this, true); }   // full editor: edits go to getEditable()

        // ---- Telling the keyboard about the editor (beyond updateSelection) ----

        /** Where the caret is on screen. Keyboards ask for this to place their own UI. */
        @Override public boolean requestCursorUpdates(int cursorUpdateMode) {
            if ((cursorUpdateMode & InputConnection.CURSOR_UPDATE_IMMEDIATE) != 0) {
                sendCursorAnchorInfo();
            }
            mMonitorCursor = (cursorUpdateMode & InputConnection.CURSOR_UPDATE_MONITOR) != 0;
            return true;
        }

        void reportCursorAnchorInfoIfMonitoring() {
            if (mMonitorCursor) {
                sendCursorAnchorInfo();
            }
        }

        private void sendCursorAnchorInfo() {
            if (mAnchorBuilder == null) {
                mAnchorBuilder = new CursorAnchorInfo.Builder();
            } else {
                mAnchorBuilder.reset();
            }
            int ss = Selection.getSelectionStart(mEditable);
            int se = Selection.getSelectionEnd(mEditable);
            mAnchorBuilder.setSelectionRange(ss, se);
            int cs = BaseInputConnection.getComposingSpanStart(mEditable);
            int ce = BaseInputConnection.getComposingSpanEnd(mEditable);
            if (cs >= 0 && ce > cs) {
                mAnchorBuilder.setComposingText(cs, mEditable.subSequence(cs, ce));
            } else {
                mAnchorBuilder.setComposingText(-1, "");
            }
            // The app's coordinates are window-relative; the IME works in screen coordinates.
            getLocationOnScreen(mScreenLocation);
            mViewToScreen.setTranslate(mScreenLocation[0], mScreenLocation[1]);
            mAnchorBuilder.setMatrix(mViewToScreen);
            if (!Float.isNaN(mCaretX)) {
                int flags = CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION | (mCaretRtl ? CursorAnchorInfo.FLAG_IS_RTL : 0);
                // No separate baseline from the toolkit: the bottom of the caret is close enough
                // for placing keyboard UI.
                mAnchorBuilder.setInsertionMarkerLocation(mCaretX, mCaretTop, mCaretBottom, mCaretBottom, flags);
            }
            mImm.updateCursorAnchorInfo(InputView.this, mAnchorBuilder.build());
        }

        /** The whole text, for fullscreen (landscape) keyboards and extracted-text monitors.
         *  Returning null, as BaseInputConnection does, makes some keyboards disable
         *  suggestions and gesture typing. */
        @Override public ExtractedText getExtractedText(ExtractedTextRequest request, int flags) {
            mExtractRequest = (flags & InputConnection.GET_EXTRACTED_TEXT_MONITOR) != 0 ? request : null;
            return buildExtractedText(request);
        }

        private ExtractedText buildExtractedText(ExtractedTextRequest request) {
            mExtracted.startOffset = 0;
            mExtracted.partialStartOffset = -1;
            mExtracted.partialEndOffset = -1;
            mExtracted.selectionStart = Selection.getSelectionStart(mEditable);
            mExtracted.selectionEnd = Selection.getSelectionEnd(mEditable);
            mExtracted.flags = (mInputType & InputType.TYPE_TEXT_FLAG_MULTI_LINE) != 0
                    ? 0 : ExtractedText.FLAG_SINGLE_LINE;
            boolean withStyles = request != null && (request.flags & InputConnection.GET_TEXT_WITH_STYLES) != 0;
            mExtracted.text = withStyles ? mEditable : mEditable.toString();
            return mExtracted;
        }

        void reportExtractedTextIfMonitoring() {
            if (mExtractRequest != null) {
                mImm.updateExtractedText(InputView.this, mExtractRequest.token,
                        buildExtractedText(mExtractRequest));
            }
        }

        @Override public void closeConnection() {
            super.closeConnection();
            mMonitorCursor = false;
            mExtractRequest = null;
        }

        @Override public Editable getEditable() { return mEditable; }

        // Batch edits: report once at the end, never a half-applied state.
        @Override public boolean beginBatchEdit() { mBatch++; return true; }
        @Override public boolean endBatchEdit() {
            if (mBatch > 0 && --mBatch == 0) report();
            return mBatch > 0;
        }
        private boolean changed(boolean result) {
            // Typing dismisses the toolbar, like EditText; keyboards' no-op calls must not. Gboard
            // answers a new selection with e.g. finishComposingText(), which killed the toolbar
            // within 50-450 ms on Android 16.
            if (!mEditable.toString().equals(mReportedText)) hideToolbar();
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
