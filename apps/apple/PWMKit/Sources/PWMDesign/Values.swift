// The value types Tokens.swift uses. Plain data, so they build and test on
// Linux too; SwiftUI.swift turns them into SwiftUI values on Apple platforms.

/// An sRGB color: 0–255 per channel, alpha 0–1.
public struct RGBA: Sendable, Equatable {
    public let red: UInt8
    public let green: UInt8
    public let blue: UInt8
    public let alpha: Double

    public init(_ red: UInt8, _ green: UInt8, _ blue: UInt8, alpha: Double = 1) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }

    /// `#RRGGBB`, for tests and debugging.
    public var hex: String {
        let digits = Array("0123456789ABCDEF")
        return "#" + [red, green, blue].map { "\(digits[Int($0 >> 4)])\(digits[Int($0 & 0xF)])" }.joined()
    }
}

/// A color with its light and dark appearance.
public struct ThemedColor: Sendable, Equatable {
    public let light: RGBA
    public let dark: RGBA
}

/// A text style: size in points, CSS weight (400 regular … 700 bold), line
/// height as a multiple of the size.
public struct TypeStyle: Sendable, Equatable {
    public let size: Double
    public let weight: Int
    public let lineHeight: Double
    public let monospaced: Bool
}
