package systems.rune.androidtest

import android.content.Context
import android.content.res.Configuration
import android.os.SystemClock
import android.text.Editable
import android.text.InputType
import android.text.Selection
import android.text.SpannableStringBuilder
import android.util.Log
import android.view.Choreographer
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowInsets
import android.view.WindowInsetsAnimation
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager

/**
 * A SurfaceView that Rune draws into through wgpu (`embed::LayerHost` on the surface's
 * `ANativeWindow`). Frames come from Choreographer, one per vsync while Rune animates or input
 * arrives; touches, keys, window insets and the soft keyboard go to the native runtime, which
 * outlives this view.
 *
 * Text input: the InputConnection edits a real [Editable] (Android does composing, surrounding
 * text, deletes and selection on it); each change is sent whole to Rune's focused field
 * (`Ime::Replace`), and after each frame the field as Rune holds it comes back, so a change
 * Rune made (a tap moving the caret, a send clearing the field) reaches the keyboard.
 */
class RuneView(context: Context) : SurfaceView(context), SurfaceHolder.Callback, Choreographer.FrameCallback {
    private var framePosted = false
    private val imm = context.getSystemService(InputMethodManager::class.java)

    /** What the keyboard edits: the focused field, with the composition in it. */
    val editable: Editable = SpannableStringBuilder()
    private var fieldHash: Int? = null
    private var fieldInfo = IntArray(0)
    private var keyboardShown = false
    private var connectionBatch = 0
    private var lastInsets = IntArray(4)

    init {
        holder.addCallback(this)
        isFocusable = true
        isFocusableInTouchMode = true
        nativeAttach()
        setOnApplyWindowInsetsListener { _, insets ->
            sendInsets(insets)
            insets
        }
        // The keyboard's inset each frame of its animation, so the composer rides up with it.
        setWindowInsetsAnimationCallback(object : WindowInsetsAnimation.Callback(DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
            override fun onProgress(insets: WindowInsets, running: MutableList<WindowInsetsAnimation>): WindowInsets {
                sendInsets(insets)
                return insets
            }
        })
    }

    private fun sendInsets(insets: WindowInsets) {
        val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
        val ime = insets.getInsets(WindowInsets.Type.ime())
        val now = intArrayOf(bars.top, bars.right, maxOf(bars.bottom, ime.bottom), bars.left)
        if (!now.contentEquals(lastInsets)) {
            lastInsets = now
            nativeInsets(now[0], now[1], now[2], now[3])
            scheduleFrame()
        }
    }

    // Surface lifecycle ----------------------------------------------------------------------

    override fun surfaceCreated(holder: SurfaceHolder) {
        val dark = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        val frame = holder.surfaceFrame
        nativeSurfaceCreated(holder.surface, frame.width(), frame.height(), resources.displayMetrics.density, dark)
        rootWindowInsets?.let { lastInsets = IntArray(4); sendInsets(it) }
        scheduleFrame()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        nativeSurfaceChanged(width, height, resources.displayMetrics.density)
        scheduleFrame()
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        nativeSurfaceDestroyed()
    }

    // Frames -------------------------------------------------------------------------------------

    /** True while the frame posted is a timer's (`wake_after`), which input may bring forward. */
    private var frameDelayed = false

    fun scheduleFrame() {
        if (framePosted && !frameDelayed) return
        val choreographer = Choreographer.getInstance()
        if (frameDelayed) choreographer.removeFrameCallback(this)
        framePosted = true
        frameDelayed = false
        choreographer.postFrameCallback(this)
    }

    /** A wake from Rune (a timer, another thread's update, an accessibility request): any thread. */
    @Suppress("unused")
    fun wakeFromNative() {
        post { scheduleFrame() }
    }

    override fun doFrame(frameTimeNanos: Long) {
        framePosted = false
        frameDelayed = false
        val flags = nativeFrame()
        if (flags and 1 != 0) scheduleFrame()
        else {
            val wake = nativeWakeAfterMs()
            if (wake >= 0) {
                framePosted = true
                frameDelayed = true
                Choreographer.getInstance().postFrameCallbackDelayed(this, wake)
            }
        }
        syncInput(flags and 2 != 0)
    }

    // Touch and keys -----------------------------------------------------------------------------

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val n = event.pointerCount
        val ids = IntArray(n) { event.getPointerId(it) }
        val xs = FloatArray(n) { event.getX(it) }
        val ys = FloatArray(n) { event.getY(it) }
        if (event.actionMasked == MotionEvent.ACTION_DOWN && !hasFocus()) requestFocus()
        nativeTouch(event.actionMasked, event.actionIndex, ids, xs, ys, event.eventTime)
        scheduleFrame()
        return true
    }

    private fun keyCode(keyCode: Int): Int = when (keyCode) {
        KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> 1
        KeyEvent.KEYCODE_DEL -> 2
        KeyEvent.KEYCODE_FORWARD_DEL -> 3
        KeyEvent.KEYCODE_DPAD_LEFT -> 4
        KeyEvent.KEYCODE_DPAD_RIGHT -> 5
        KeyEvent.KEYCODE_DPAD_UP -> 6
        KeyEvent.KEYCODE_DPAD_DOWN -> 7
        KeyEvent.KEYCODE_ESCAPE -> 8
        else -> 0
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK) return super.onKeyDown(keyCode, event)
        val code = keyCode(keyCode)
        val ch = event.unicodeChar
        if (code == 0 && ch == 0) return super.onKeyDown(keyCode, event)
        nativeKey(code, if (code == 0) String(Character.toChars(ch)) else "", event.isShiftPressed)
        scheduleFrame()
        return true
    }

    // Text input ---------------------------------------------------------------------------------

    override fun onCheckIsTextEditor(): Boolean = fieldHash != null

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
        val info = fieldInfo
        if (info.size < 8) return null
        val singleLine = info[4] != 0
        val secure = info[5] != 0
        val submits = info[6] != 0
        var type = InputType.TYPE_CLASS_TEXT
        type = if (secure) type or InputType.TYPE_TEXT_VARIATION_PASSWORD
        else type or InputType.TYPE_TEXT_FLAG_AUTO_CORRECT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
        if (!singleLine) type = type or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        outAttrs.inputType = type
        outAttrs.imeOptions = (if (submits) EditorInfo.IME_ACTION_SEND else EditorInfo.IME_ACTION_DONE) or
            EditorInfo.IME_FLAG_NO_FULLSCREEN or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        outAttrs.initialSelStart = Selection.getSelectionStart(editable)
        outAttrs.initialSelEnd = Selection.getSelectionEnd(editable)
        outAttrs.setInitialSurroundingText(editable)
        return Connection()
    }

    /** Sends the Editable to Rune's field after every change the keyboard makes. */
    private inner class Connection : BaseInputConnection(this@RuneView, true) {
        override fun getEditable(): Editable = this@RuneView.editable

        override fun beginBatchEdit(): Boolean { connectionBatch++; return true }
        override fun endBatchEdit(): Boolean {
            if (connectionBatch > 0) connectionBatch--
            if (connectionBatch == 0) pushEditable()
            return connectionBatch > 0
        }

        private fun <T> edit(f: () -> T): T { val r = f(); if (connectionBatch == 0) pushEditable(); return r }

        override fun commitText(text: CharSequence?, newCursorPosition: Int) = edit { super.commitText(text, newCursorPosition) }
        override fun setComposingText(text: CharSequence?, newCursorPosition: Int) = edit { super.setComposingText(text, newCursorPosition) }
        override fun setComposingRegion(start: Int, end: Int) = edit { super.setComposingRegion(start, end) }
        override fun finishComposingText() = edit { super.finishComposingText() }
        override fun deleteSurroundingText(beforeLength: Int, afterLength: Int) = edit { super.deleteSurroundingText(beforeLength, afterLength) }
        override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int) =
            edit { super.deleteSurroundingTextInCodePoints(beforeLength, afterLength) }
        override fun setSelection(start: Int, end: Int) = edit { super.setSelection(start, end) }
        override fun replaceText(start: Int, end: Int, text: CharSequence, newCursorPosition: Int, textAttribute: android.view.inputmethod.TextAttribute?) =
            edit { super.replaceText(start, end, text, newCursorPosition, textAttribute) }

        override fun performEditorAction(actionCode: Int): Boolean {
            Log.i(TAG, "editor action $actionCode")
            nativeKey(1, "", false)
            scheduleFrame()
            return true
        }
    }

    private fun pushEditable() {
        val text = editable.toString()
        val s0 = Selection.getSelectionStart(editable)
        val s1 = Selection.getSelectionEnd(editable)
        val c0 = BaseInputConnection.getComposingSpanStart(editable)
        val c1 = BaseInputConnection.getComposingSpanEnd(editable)
        nativeSetField(text, s0, s1, c0, c1)
        scheduleFrame()
    }

    /** After a frame: show or hide the keyboard as a field gains or loses focus, and bring a
     *  change Rune made to the field into the Editable the keyboard reads. */
    private fun syncInput(focused: Boolean) {
        if (!focused) {
            if (fieldHash != null) {
                fieldHash = null
                fieldInfo = IntArray(0)
                editable.clear()
                imm.restartInput(this)
                if (keyboardShown) imm.hideSoftInputFromWindow(windowToken, 0)
                keyboardShown = false
            }
            return
        }
        if (connectionBatch > 0) return
        val text = nativeFieldText() ?: return
        val info = nativeFieldInfo()
        if (info.size < 8) return
        val newField = fieldHash != info[7]
        fieldInfo = info
        val c0 = BaseInputConnection.getComposingSpanStart(editable)
        val c1 = BaseInputConnection.getComposingSpanEnd(editable)
        val sameText = editable.toString() == text
        val sameSel = Selection.getSelectionStart(editable) == info[0] && Selection.getSelectionEnd(editable) == info[1]
        val sameComp = c0 == info[2] && c1 == info[3]
        if (newField || !sameText) {
            editable.replace(0, editable.length, text)
            BaseInputConnection.removeComposingSpans(editable)
            Selection.setSelection(editable, info[0].coerceIn(0, text.length), info[1].coerceIn(0, text.length))
            fieldHash = info[7]
            if (!hasFocus()) requestFocus()
            imm.restartInput(this)
        } else if (!sameSel || !sameComp) {
            Selection.setSelection(editable, info[0].coerceIn(0, text.length), info[1].coerceIn(0, text.length))
            imm.updateSelection(this, info[0], info[1], info[2], info[3])
        }
        if (!keyboardShown) {
            if (!hasFocus()) requestFocus()
            imm.showSoftInput(this, 0)
            keyboardShown = true
        }
    }

    // Native ----------------------------------------------------------------------------------

    private external fun nativeAttach()
    private external fun nativeSurfaceCreated(surface: android.view.Surface, width: Int, height: Int, density: Float, dark: Boolean)
    private external fun nativeSurfaceChanged(width: Int, height: Int, density: Float)
    private external fun nativeSurfaceDestroyed()
    private external fun nativeTouch(action: Int, index: Int, ids: IntArray, xs: FloatArray, ys: FloatArray, timeMs: Long)
    private external fun nativeKey(code: Int, text: String, shift: Boolean)
    private external fun nativeSetField(text: String, selStart: Int, selEnd: Int, compStart: Int, compEnd: Int)
    private external fun nativeInsets(top: Int, right: Int, bottom: Int, left: Int)
    private external fun nativeFrame(): Int
    private external fun nativeWakeAfterMs(): Long
    private external fun nativeFieldText(): String?
    private external fun nativeFieldInfo(): IntArray

    companion object {
        const val TAG = "RuneView"
        init { System.loadLibrary("android_view") }
    }
}
