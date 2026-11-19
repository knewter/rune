package systems.rune.androidtest

import android.app.Instrumentation
import android.app.UiAutomation
import android.content.Intent
import android.graphics.Bitmap
import android.os.Bundle
import android.util.Log
import android.view.accessibility.AccessibilityNodeInfo
import java.io.File

/**
 * An accessibility client for checking Rune's AccessKit tree on a device, since adb's injected
 * touches and keys bypass TalkBack's own gestures. It acts on nodes as TalkBack does
 * (`performAction`), with TalkBack left running, and saves a screenshot after each step:
 *
 *   adb shell am instrument -w -e steps "focus:Tap,click:Tap,click:Message" \
 *       systems.rune.androidtest/.A11yProbe
 *   adb exec-out run-as systems.rune.androidtest cat files/probe-<n>.png
 *
 * A step is `focus` (accessibility focus, where TalkBack reads), `click` (TalkBack's
 * double-tap) or `read` (log the node), on the node whose description is the name given.
 */
class A11yProbe : Instrumentation() {
    private var steps = ""

    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        steps = arguments?.getString("steps").orEmpty()
        start()
    }

    override fun onStart() {
        val out = Bundle()
        val ui = getUiAutomation(UiAutomation.FLAG_DONT_SUPPRESS_ACCESSIBILITY_SERVICES)
        targetContext.startActivity(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        waitForIdleSync()
        Thread.sleep(1500)
        var n = 0
        for (step in steps.split(',').filter { it.isNotBlank() }) {
            val (verb, name) = step.split(':', limit = 2).let { it[0] to it.getOrElse(1) { "" } }
            val node = find(ui.rootInActiveWindow, name)
            val line = if (node == null) "$step: no node" else {
                val ok = when (verb) {
                    "focus" -> node.performAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS)
                    "click" -> node.performAction(AccessibilityNodeInfo.ACTION_CLICK)
                    else -> true
                }
                "$step: ${node.className} \"${node.contentDescription}\" actions=${node.actionList.map { it.id }} -> $ok"
            }
            Log.i(TAG, line)
            out.putString("step$n", line)
            Thread.sleep(1200)
            ui.takeScreenshot()?.let { shot ->
                File(targetContext.filesDir, "probe-$n.png").outputStream().use { shot.compress(Bitmap.CompressFormat.PNG, 100, it) }
            }
            n++
        }
        finish(0, out)
    }

    /** The node named `name`, preferring one that acts (a button or field) to its text. */
    private fun find(root: AccessibilityNodeInfo?, name: String): AccessibilityNodeInfo? {
        root ?: return null
        val all = ArrayList<AccessibilityNodeInfo>()
        fun walk(n: AccessibilityNodeInfo) {
            all.add(n)
            for (i in 0 until n.childCount) n.getChild(i)?.let { walk(it) }
        }
        walk(root)
        val named = all.filter { it.contentDescription?.toString() == name || it.text?.toString() == name }
        return named.firstOrNull { it.actionList.any { a -> a.id == AccessibilityNodeInfo.ACTION_CLICK } } ?: named.firstOrNull()
    }

    companion object { const val TAG = "RuneA11yProbe" }
}
