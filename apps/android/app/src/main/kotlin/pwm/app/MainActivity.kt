package pwm.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge

/** The app's one Activity (D53 §1): it draws whichever screen the model shows. */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val model = (application as PwmApplication).model
        setContent { PwmTheme { AppScreen(model) } }
    }
}
