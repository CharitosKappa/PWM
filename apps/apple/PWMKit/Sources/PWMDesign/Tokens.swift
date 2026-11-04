// Generated from design/tokens.json by scripts/tokens-swift.mjs. Do not edit.

/// The shared design tokens (D41 §5). Placeholder values until D1.
public enum Tokens {
    public static let version = 1

    public enum Color {
        public static let bg = ThemedColor(light: RGBA(0xFF, 0xFF, 0xFF, alpha: 1), dark: RGBA(0x0F, 0x11, 0x15, alpha: 1))
        public static let surface = ThemedColor(light: RGBA(0xF5, 0xF6, 0xF8, alpha: 1), dark: RGBA(0x18, 0x1B, 0x21, alpha: 1))
        public static let surfaceRaised = ThemedColor(light: RGBA(0xFF, 0xFF, 0xFF, alpha: 1), dark: RGBA(0x20, 0x24, 0x2C, alpha: 1))
        public static let border = ThemedColor(light: RGBA(0xD9, 0xDC, 0xE1, alpha: 1), dark: RGBA(0x2E, 0x33, 0x3D, alpha: 1))
        public static let text = ThemedColor(light: RGBA(0x14, 0x17, 0x1C, alpha: 1), dark: RGBA(0xEC, 0xEE, 0xF2, alpha: 1))
        public static let textMuted = ThemedColor(light: RGBA(0x5B, 0x62, 0x70, alpha: 1), dark: RGBA(0x9A, 0xA1, 0xAE, alpha: 1))
        public static let accent = ThemedColor(light: RGBA(0x24, 0x57, 0xD6, alpha: 1), dark: RGBA(0x6F, 0x95, 0xF5, alpha: 1))
        public static let accentText = ThemedColor(light: RGBA(0xFF, 0xFF, 0xFF, alpha: 1), dark: RGBA(0x0F, 0x11, 0x15, alpha: 1))
        public static let focus = ThemedColor(light: RGBA(0x24, 0x57, 0xD6, alpha: 1), dark: RGBA(0x6F, 0x95, 0xF5, alpha: 1))
        public static let success = ThemedColor(light: RGBA(0x1E, 0x7F, 0x4F, alpha: 1), dark: RGBA(0x4C, 0xC3, 0x8A, alpha: 1))
        public static let warning = ThemedColor(light: RGBA(0x9A, 0x61, 0x00, alpha: 1), dark: RGBA(0xE2, 0xA3, 0x36, alpha: 1))
        public static let danger = ThemedColor(light: RGBA(0xC4, 0x2B, 0x2B, alpha: 1), dark: RGBA(0xF0, 0x70, 0x70, alpha: 1))
    }

    /// Sizes are the web sizes in points; SwiftUI scales them with Dynamic Type.
    public enum Text {
        public static let caption = TypeStyle(size: 12, weight: 400, lineHeight: 1.33, monospaced: false)
        public static let body = TypeStyle(size: 14, weight: 400, lineHeight: 1.43, monospaced: false)
        public static let bodyStrong = TypeStyle(size: 14, weight: 600, lineHeight: 1.43, monospaced: false)
        public static let title = TypeStyle(size: 17, weight: 600, lineHeight: 1.29, monospaced: false)
        public static let headline = TypeStyle(size: 22, weight: 600, lineHeight: 1.27, monospaced: false)
        public static let display = TypeStyle(size: 28, weight: 600, lineHeight: 1.21, monospaced: false)
        public static let secret = TypeStyle(size: 15, weight: 400, lineHeight: 1.4, monospaced: true)
    }

    public enum Space {
        public static let s0: Double = 0
        public static let s1: Double = 4
        public static let s2: Double = 8
        public static let s3: Double = 12
        public static let s4: Double = 16
        public static let s5: Double = 24
        public static let s6: Double = 32
        public static let s7: Double = 48
    }

    public enum Radius {
        public static let none: Double = 0
        public static let sm: Double = 4
        public static let md: Double = 8
        public static let lg: Double = 12
        public static let full: Double = 9999
    }
}
