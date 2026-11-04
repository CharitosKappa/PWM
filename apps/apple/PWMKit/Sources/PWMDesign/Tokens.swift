// Generated from design/tokens.json by scripts/tokens-swift.mjs. Do not edit.

/// The shared design tokens (D41 §5). Placeholder values until D1.
public enum Tokens {
    public static let version = 1

    public enum Color {
        public static let bg = ThemedColor(light: RGBA(0xFF, 0xFF, 0xFF, alpha: 1), dark: RGBA(0x13, 0x12, 0x1F, alpha: 1))
        public static let surface = ThemedColor(light: RGBA(0xF4, 0xF3, 0xFB, alpha: 1), dark: RGBA(0x1C, 0x1B, 0x2C, alpha: 1))
        public static let surfaceRaised = ThemedColor(light: RGBA(0xFF, 0xFF, 0xFF, alpha: 1), dark: RGBA(0x25, 0x24, 0x38, alpha: 1))
        public static let border = ThemedColor(light: RGBA(0xE2, 0xE0, 0xF0, alpha: 1), dark: RGBA(0x34, 0x32, 0x4A, alpha: 1))
        public static let text = ThemedColor(light: RGBA(0x1E, 0x1B, 0x3A, alpha: 1), dark: RGBA(0xEE, 0xED, 0xF7, alpha: 1))
        public static let textMuted = ThemedColor(light: RGBA(0x5D, 0x5A, 0x78, alpha: 1), dark: RGBA(0xA9, 0xA6, 0xC4, alpha: 1))
        public static let accent = ThemedColor(light: RGBA(0x4F, 0x3F, 0xD6, alpha: 1), dark: RGBA(0xA7, 0x9B, 0xFF, alpha: 1))
        public static let accentText = ThemedColor(light: RGBA(0xFF, 0xFF, 0xFF, alpha: 1), dark: RGBA(0x13, 0x12, 0x1F, alpha: 1))
        public static let accentSoft = ThemedColor(light: RGBA(0xEC, 0xEA, 0xFD, alpha: 1), dark: RGBA(0x2A, 0x26, 0x50, alpha: 1))
        public static let focus = ThemedColor(light: RGBA(0x4F, 0x3F, 0xD6, alpha: 1), dark: RGBA(0xA7, 0x9B, 0xFF, alpha: 1))
        public static let success = ThemedColor(light: RGBA(0x1C, 0x7C, 0x54, alpha: 1), dark: RGBA(0x5F, 0xD3, 0x9A, alpha: 1))
        public static let warning = ThemedColor(light: RGBA(0x8A, 0x5A, 0x00, alpha: 1), dark: RGBA(0xF0, 0xB4, 0x4C, alpha: 1))
        public static let danger = ThemedColor(light: RGBA(0xC0, 0x30, 0x4A, alpha: 1), dark: RGBA(0xFF, 0x80, 0x95, alpha: 1))
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
        public static let sm: Double = 12
        public static let md: Double = 16
        public static let lg: Double = 20
        public static let full: Double = 9999
    }
}
