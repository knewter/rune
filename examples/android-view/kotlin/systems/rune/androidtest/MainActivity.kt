package systems.rune.androidtest

import android.app.Activity
import android.content.res.Configuration
import android.os.Bundle
import android.view.WindowInsetsController

/** An ordinary activity whose only content is a [RuneView]. Rune's runtime lives in the
 *  native library for the life of the process, so this activity can be destroyed and made
 *  again (Back, rotation) and the view it makes attaches to the same runtime. */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Edge to edge: Rune draws under the system bars and keyboard and pads its content by
        // the insets RuneView reports (Cx::safe_area).
        window.setDecorFitsSystemWindows(false)
        window.statusBarColor = android.graphics.Color.TRANSPARENT
        window.navigationBarColor = android.graphics.Color.TRANSPARENT
        // Dark system-bar icons over Rune's light theme, light ones over its dark theme.
        val dark = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        val light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
        window.insetsController?.setSystemBarsAppearance(if (dark) 0 else light, light)
        setContentView(RuneView(this))
    }
}
