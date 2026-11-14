// oxydroid: a native text-selection handle (caret, selection start or end).
// Licensed under MIT OR Apache-2.0.
//
// Uses the platform's own handle drawables, in a PopupWindow so it sits (and is touchable) above
// the native surface. The app positions it by its tip (window pixels); drags are reported back to
// Rust, which moves the caret or selection with its own layout and repositions the handle.
// Only touched on the main thread.

package dev.kotbyte.oxydroid;

import android.content.res.TypedArray;
import android.graphics.PorterDuff;
import android.graphics.drawable.Drawable;
import android.view.Gravity;
import android.view.MotionEvent;
import android.widget.ImageView;
import android.widget.PopupWindow;

final class Handle extends ImageView {
    static final int CARET = 0;
    static final int START = 1;
    static final int END = 2;
    static final int PHASE_DOWN = 0;
    static final int PHASE_MOVE = 1;
    static final int PHASE_UP = 2;

    private final InputView mOwner;
    private final int mId;
    private final PopupWindow mPopup;
    /** Where the tip is within the drawable. The start handle points up-right, the end handle
     *  up-left, the caret handle straight up: the tip sits at 3/4, 1/4 and 1/2 of the width. */
    private final int mHotspotX;
    private int mTipX;
    private int mTipY;
    private float mGrabX;
    private float mGrabY;
    private final int[] mOrigin = new int[2];

    Handle(InputView owner, int id) {
        super(owner.getContext());
        mOwner = owner;
        mId = id;
        int attr = id == START ? android.R.attr.textSelectHandleLeft
                : id == END ? android.R.attr.textSelectHandleRight
                : android.R.attr.textSelectHandle;
        TypedArray a = getContext().getTheme().obtainStyledAttributes(new int[] { attr });
        Drawable d = a.getDrawable(0);
        a.recycle();
        setImageDrawable(d);
        int w = d.getIntrinsicWidth();
        int h = d.getIntrinsicHeight();
        mHotspotX = id == START ? w * 3 / 4 : id == END ? w / 4 : w / 2;
        mPopup = new PopupWindow(this, w, h, false);
        mPopup.setClippingEnabled(false);   // may hang below the window's content area
        mPopup.setSplitTouchEnabled(true);  // drag both selection handles at once
    }

    void setColor(int argb) {
        if (argb != 0) setColorFilter(argb, PorterDuff.Mode.SRC_IN);
    }

    /** Put the tip at (x, y), window pixels: usually the caret's x at the bottom of its line. */
    void place(int x, int y) {
        mTipX = x;
        mTipY = y;
        int left = x - mHotspotX;
        if (mPopup.isShowing()) {
            mPopup.update(left, y, -1, -1);
        } else {
            mPopup.showAtLocation(mOwner, Gravity.NO_GRAVITY, left, y);
        }
    }

    void hide() {
        mPopup.dismiss();
    }

    boolean isShowing() {
        return mPopup.isShowing();
    }

    int tipX() {
        return mTipX;
    }

    int tipY() {
        return mTipY;
    }

    @Override public boolean onTouchEvent(MotionEvent e) {
        int phase;
        switch (e.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                // Remember where in the handle the finger grabbed it, so the tip doesn't jump.
                mOwner.getLocationOnScreen(mOrigin);
                mGrabX = e.getRawX() - (mOrigin[0] + mTipX);
                mGrabY = e.getRawY() - (mOrigin[1] + mTipY);
                phase = PHASE_DOWN;
                break;
            case MotionEvent.ACTION_MOVE:
                phase = PHASE_MOVE;
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_CANCEL:
                phase = PHASE_UP;
                break;
            default:
                return true;
        }
        // Where the tip should be now, in window pixels.
        float x = e.getRawX() - mGrabX - mOrigin[0];
        float y = e.getRawY() - mGrabY - mOrigin[1];
        mOwner.onHandleDrag(this, x, y, phase);
        Platform.nativeOnHandle(mId, x, y, phase);
        return true;
    }
}
