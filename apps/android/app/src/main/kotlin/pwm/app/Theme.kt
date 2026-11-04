package pwm.app

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import pwm.kit.design.ThemedColor
import pwm.kit.design.Tokens
import pwm.kit.design.TypeStyle

/** A design token's color for the system's light or dark mode. */
@Composable
fun color(token: ThemedColor): Color = Color(if (isSystemInDarkTheme()) token.dark else token.light)

fun TypeStyle.compose(): TextStyle = TextStyle(
    fontSize = size.sp,
    lineHeight = (size * lineHeight).sp,
    fontWeight = FontWeight(weight),
    fontFamily = if (monospaced) FontFamily.Monospace else FontFamily.Default,
)

/** Material 3, colored and set from `design/tokens.json` (D52 §6). */
@Composable
fun PwmTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    fun pick(token: ThemedColor) = Color(if (dark) token.dark else token.light)
    val c = Tokens.Color
    val scheme = (if (dark) darkColorScheme() else lightColorScheme()).copy(
        primary = pick(c.accent),
        onPrimary = pick(c.accentText),
        background = pick(c.bg),
        onBackground = pick(c.text),
        surface = pick(c.bg),
        onSurface = pick(c.text),
        surfaceVariant = pick(c.surface),
        onSurfaceVariant = pick(c.textMuted),
        outline = pick(c.border),
        error = pick(c.danger),
    )
    val base = MaterialTheme.typography
    val t = Tokens.Text
    val typography = base.copy(
        headlineMedium = t.display.compose(),
        titleLarge = t.headline.compose(),
        titleMedium = t.title.compose(),
        bodyLarge = t.body.compose(),
        bodyMedium = t.body.compose(),
        bodySmall = t.caption.compose(),
        labelLarge = t.bodyStrong.compose(),
    )
    MaterialTheme(colorScheme = scheme, typography = typography, content = content)
}
