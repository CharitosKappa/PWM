package pwm.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import pwm.kit.design.ThemedColor
import pwm.kit.design.Tokens
import pwm.kit.design.TypeStyle
import uniffi.pwm_ffi.newId

/**
 * The app's one screen until M6.6 (D52 §1): it shows that the core loaded
 * on this device. Onboarding and the vault replace it.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { Placeholder(coreLoaded = runCatching { newId() }.isSuccess) }
    }
}

@Composable
fun color(token: ThemedColor): Color = Color(if (isSystemInDarkTheme()) token.dark else token.light)

fun TypeStyle.compose(): TextStyle = TextStyle(
    fontSize = size.sp,
    lineHeight = (size * lineHeight).sp,
    fontWeight = FontWeight(weight),
    fontFamily = if (monospaced) FontFamily.Monospace else FontFamily.Default,
)

@Composable
fun Placeholder(coreLoaded: Boolean) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(color(Tokens.Color.bg))
            .padding(Tokens.Space.s5.dp),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.s3.dp),
    ) {
        Text("PWM", style = Tokens.Text.display.compose(), color = color(Tokens.Color.text))
        Text(
            if (coreLoaded) "The core is ready. Sign-in comes next." else "The core didn't load on this device.",
            style = Tokens.Text.body.compose(),
            color = color(if (coreLoaded) Tokens.Color.textMuted else Tokens.Color.danger),
            modifier = Modifier.testTag("coreStatus"),
        )
    }
}
