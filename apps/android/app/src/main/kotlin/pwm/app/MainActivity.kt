package pwm.app

import android.os.Build
import android.content.Intent
import android.os.Bundle
import android.view.WindowManager
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.fragment.app.FragmentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.ui.platform.LocalView
import pwm.kit.ui.Announcement
import pwm.kit.ui.Speech

/**
 * The app's one Activity (D53 §1): it draws whichever screen the model
 * shows. A `FragmentActivity`, which the fingerprint prompt needs (D55 §5).
 */
class MainActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val model = (application as PwmApplication).model
        openFrom(intent)
        setContent {
            val state by model.state.collectAsState()
            // One place for the window's capture rules (D56 §4).
            LaunchedEffect(state.secureWindow, state.hideFromRecents) {
                if (state.secureWindow) {
                    window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
                } else {
                    window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
                }
                // Android 13+: no picture in the app switcher, even with the switch off (D56 §3).
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) setRecentsScreenshotEnabled(!state.hideFromRecents)
            }
            // Copies and locking are said without moving focus (design/README.md, Accessibility).
            val view = LocalView.current
            DisposableEffect(view) {
                Speech.listener = { announcement ->
                    val line = getString(
                        when (announcement) {
                            Announcement.COPIED -> R.string.announce_copied
                            Announcement.LOCKED -> R.string.announce_locked
                        },
                    )
                    // Deprecated on Android 16 in favour of live regions, which need a
                    // node on screen; the lock replaces the whole screen, so this stays.
                    @Suppress("DEPRECATION")
                    view.post { view.announceForAccessibility(line) }
                }
                onDispose { Speech.listener = {} }
            }
            PwmTheme { AppScreen(model) }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openFrom(intent)
    }

    /** The new-device notice opens "Approve a new device" (D60 §4), after unlocking if need be. */
    private fun openFrom(intent: Intent?) {
        if (intent?.action == AndroidNotices.OPEN_APPROVALS) (application as PwmApplication).model.openApprovals()
    }
}
