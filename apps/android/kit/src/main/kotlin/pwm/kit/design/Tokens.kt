// Generated from design/tokens.json by scripts/tokens-kotlin.mjs. Do not edit.
package pwm.kit.design

/** A color for light and dark themes, as `0xAARRGGBB`. */
data class ThemedColor(val light: Long, val dark: Long)

/** A text style: size in sp, weight 100 to 900, line height as a multiple. */
data class TypeStyle(val size: Double, val weight: Int, val lineHeight: Double, val monospaced: Boolean)

/** The shared design tokens (D52 §6). Placeholder values until D1. */
object Tokens {
    const val VERSION = 1

    object Color {
        val bg = ThemedColor(light = 0xFFFFFFFF, dark = 0xFF0F1115)
        val surface = ThemedColor(light = 0xFFF5F6F8, dark = 0xFF181B21)
        val surfaceRaised = ThemedColor(light = 0xFFFFFFFF, dark = 0xFF20242C)
        val border = ThemedColor(light = 0xFFD9DCE1, dark = 0xFF2E333D)
        val text = ThemedColor(light = 0xFF14171C, dark = 0xFFECEEF2)
        val textMuted = ThemedColor(light = 0xFF5B6270, dark = 0xFF9AA1AE)
        val accent = ThemedColor(light = 0xFF2457D6, dark = 0xFF6F95F5)
        val accentText = ThemedColor(light = 0xFFFFFFFF, dark = 0xFF0F1115)
        val focus = ThemedColor(light = 0xFF2457D6, dark = 0xFF6F95F5)
        val success = ThemedColor(light = 0xFF1E7F4F, dark = 0xFF4CC38A)
        val warning = ThemedColor(light = 0xFF9A6100, dark = 0xFFE2A336)
        val danger = ThemedColor(light = 0xFFC42B2B, dark = 0xFFF07070)
    }

    /** Sizes are the web sizes in sp; Android scales them with the font size setting. */
    object Text {
        val caption = TypeStyle(size = 12.0, weight = 400, lineHeight = 1.33, monospaced = false)
        val body = TypeStyle(size = 14.0, weight = 400, lineHeight = 1.43, monospaced = false)
        val bodyStrong = TypeStyle(size = 14.0, weight = 600, lineHeight = 1.43, monospaced = false)
        val title = TypeStyle(size = 17.0, weight = 600, lineHeight = 1.29, monospaced = false)
        val headline = TypeStyle(size = 22.0, weight = 600, lineHeight = 1.27, monospaced = false)
        val display = TypeStyle(size = 28.0, weight = 600, lineHeight = 1.21, monospaced = false)
        val secret = TypeStyle(size = 15.0, weight = 400, lineHeight = 1.4, monospaced = true)
    }

    /** In dp. */
    object Space {
        const val s0: Double = 0.0
        const val s1: Double = 4.0
        const val s2: Double = 8.0
        const val s3: Double = 12.0
        const val s4: Double = 16.0
        const val s5: Double = 24.0
        const val s6: Double = 32.0
        const val s7: Double = 48.0
    }

    /** In dp. */
    object Radius {
        const val none: Double = 0.0
        const val sm: Double = 4.0
        const val md: Double = 8.0
        const val lg: Double = 12.0
        const val full: Double = 9999.0
    }
}
