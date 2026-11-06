package pwm.app

import android.content.Context
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.font.createFontFamilyResolver
import androidx.compose.ui.unit.sp
import pwm.kit.design.ThemedColor
import pwm.kit.design.Tokens
import pwm.kit.design.TypeStyle

/** A design token's color for the system's light or dark mode. */
@Composable
fun color(token: ThemedColor): Color = Color(if (isSystemInDarkTheme()) token.dark else token.light)

// The app fonts (D1): res/font holds copies of design/fonts made by
// scripts/fonts.mjs. Both are variable fonts; each weight sets the wght axis.
private val weights = listOf(FontWeight.W400, FontWeight.W500, FontWeight.W600, FontWeight.W700)

/** Manrope, for all text. */
val Manrope = FontFamily(weights.map { Font(R.font.manrope, it) })

/** Geist Mono, for passwords, one-time codes and the Secret Key. */
val GeistMono = FontFamily(weights.map { Font(R.font.geist_mono, it) })

/**
 * Loads every weight of both fonts into Compose's process-wide typeface
 * cache. Compose otherwise loads a resource font on the main thread the
 * first time a text uses it, which made the first screen (often the fill
 * sheet) slower to show. Called once, off the main thread, at start-up.
 */
fun preloadFonts(context: Context) {
    val resolver = createFontFamilyResolver(context)
    for (family in listOf(Manrope, GeistMono)) {
        for (weight in weights) runCatching { resolver.resolve(family, weight) }
    }
}

fun TypeStyle.compose(): TextStyle = TextStyle(
    fontSize = size.sp,
    lineHeight = (size * lineHeight).sp,
    fontWeight = FontWeight(weight),
    fontFamily = if (monospaced) GeistMono else Manrope,
)

/** Every Material text style in Manrope, so no text falls back to Roboto. */
private fun Typography.inManrope(): Typography = Typography(
    displayLarge = displayLarge.copy(fontFamily = Manrope),
    displayMedium = displayMedium.copy(fontFamily = Manrope),
    displaySmall = displaySmall.copy(fontFamily = Manrope),
    headlineLarge = headlineLarge.copy(fontFamily = Manrope),
    headlineMedium = headlineMedium.copy(fontFamily = Manrope),
    headlineSmall = headlineSmall.copy(fontFamily = Manrope),
    titleLarge = titleLarge.copy(fontFamily = Manrope),
    titleMedium = titleMedium.copy(fontFamily = Manrope),
    titleSmall = titleSmall.copy(fontFamily = Manrope),
    bodyLarge = bodyLarge.copy(fontFamily = Manrope),
    bodyMedium = bodyMedium.copy(fontFamily = Manrope),
    bodySmall = bodySmall.copy(fontFamily = Manrope),
    labelLarge = labelLarge.copy(fontFamily = Manrope),
    labelMedium = labelMedium.copy(fontFamily = Manrope),
    labelSmall = labelSmall.copy(fontFamily = Manrope),
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
    val base = MaterialTheme.typography.inManrope()
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
