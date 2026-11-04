package pwm.app

import android.os.Bundle
import androidx.fragment.app.FragmentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge

/**
 * The app's one Activity (D53 §1): it draws whichever screen the model
 * shows. A `FragmentActivity`, which the fingerprint prompt needs (D55 §5).
 */
class MainActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val model = (application as PwmApplication).model
        setContent { PwmTheme { AppScreen(model) } }
    }
}
