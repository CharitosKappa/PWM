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
        val bg = ThemedColor(light = 0xFFFFFFFF, dark = 0xFF13121F)
        val surface = ThemedColor(light = 0xFFF4F3FB, dark = 0xFF1C1B2C)
        val surfaceRaised = ThemedColor(light = 0xFFFFFFFF, dark = 0xFF252438)
        val border = ThemedColor(light = 0xFFE2E0F0, dark = 0xFF34324A)
        val text = ThemedColor(light = 0xFF1E1B3A, dark = 0xFFEEEDF7)
        val textMuted = ThemedColor(light = 0xFF5D5A78, dark = 0xFFA9A6C4)
        val accent = ThemedColor(light = 0xFF4F3FD6, dark = 0xFFA79BFF)
        val accentText = ThemedColor(light = 0xFFFFFFFF, dark = 0xFF13121F)
        val accentSoft = ThemedColor(light = 0xFFECEAFD, dark = 0xFF2A2650)
        val focus = ThemedColor(light = 0xFF4F3FD6, dark = 0xFFA79BFF)
        val success = ThemedColor(light = 0xFF1C7C54, dark = 0xFF5FD39A)
        val warning = ThemedColor(light = 0xFF8A5A00, dark = 0xFFF0B44C)
        val danger = ThemedColor(light = 0xFFC0304A, dark = 0xFFFF8095)
    }

    /** The bundled fonts (D1): each family's first name. */
    object FontFamily {
        const val sans = "Manrope"
        const val mono = "Geist Mono"
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
        const val sm: Double = 12.0
        const val md: Double = 16.0
        const val lg: Double = 20.0
        const val full: Double = 9999.0
    }
}
