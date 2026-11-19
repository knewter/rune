package systems.rune.androidtest

import android.app.Activity
import android.app.UiModeManager
import android.view.ContextThemeWrapper
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
        // Debug extras that stand in for system settings, so a test needn't change the phone's:
        // `night` (yes/no/auto) sets this app's own night mode, which recreates the activity in
        // it; `fontScale` gives the view a configuration with that font scale.
        intent.getStringExtra("night")?.let {
            val mode = when (it) { "yes" -> UiModeManager.MODE_NIGHT_YES; "no" -> UiModeManager.MODE_NIGHT_NO; else -> UiModeManager.MODE_NIGHT_AUTO }
            intent.removeExtra("night")
            getSystemService(UiModeManager::class.java).setApplicationNightMode(mode)
        }
        val fontScale = intent.getFloatExtra("fontScale", 0f)
        val context = if (fontScale > 0f) ContextThemeWrapper(this, theme).apply {
            applyOverrideConfiguration(android.content.res.Configuration().apply { this.fontScale = fontScale })
        } else this
        setContentView(RuneView(context))
    }
}
