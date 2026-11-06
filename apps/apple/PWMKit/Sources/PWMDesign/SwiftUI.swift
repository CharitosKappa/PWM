// The tokens as SwiftUI values. Colors follow the system's light or dark
// appearance; text sizes scale with Dynamic Type.
#if canImport(SwiftUI)
import SwiftUI

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

extension RGBA {
    #if canImport(UIKit)
    var platform: UIColor {
        UIColor(red: Double(red) / 255, green: Double(green) / 255, blue: Double(blue) / 255, alpha: alpha)
    }
    #elseif canImport(AppKit)
    var platform: NSColor {
        NSColor(srgbRed: Double(red) / 255, green: Double(green) / 255, blue: Double(blue) / 255, alpha: alpha)
    }
    #endif
}

extension ThemedColor {
    /// The light or dark value, chosen when the appearance changes.
    public var color: Color {
        #if canImport(UIKit)
        let light = light.platform
        let dark = dark.platform
        return Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? dark : light })
        #elseif canImport(AppKit)
        let light = light.platform
        let dark = dark.platform
        return Color(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
        })
        #endif
    }
}

extension TypeStyle {
    var fontWeight: Font.Weight {
        switch weight {
        case ..<450: .regular
        case ..<550: .medium
        case ..<650: .semibold
        default: .bold
        }
    }

    /// The system text style a size scales with.
    var textStyle: Font.TextStyle {
        switch size {
        case ..<13: .caption
        case ..<16: .body
        case ..<20: .headline
        case ..<26: .title2
        default: .title
        }
    }
}

/// Applies a token text style, scaled with Dynamic Type.
private struct TokenText: ViewModifier {
    let style: TypeStyle
    @ScaledMetric private var size: Double

    init(_ style: TypeStyle) {
        AppFonts.register()
        self.style = style
        _size = ScaledMetric(wrappedValue: style.size, relativeTo: style.textStyle)
    }

    func body(content: Content) -> some View {
        content
            // The size is already scaled, so the font must not scale it again.
            .font(.custom(style.monospaced ? AppFonts.mono : AppFonts.sans, fixedSize: size).weight(style.fontWeight))
            .lineSpacing(size * (style.lineHeight - 1))
    }
}

extension Font.TextStyle {
    /// The system's size for this style at the default text size.
    var defaultSize: CGFloat {
        #if os(macOS)
        switch self {
        case .largeTitle: 26
        case .title: 22
        case .title2: 17
        case .title3: 15
        case .headline, .body: 13
        case .callout: 12
        case .subheadline, .footnote: 11
        case .caption, .caption2: 10
        @unknown default: 13
        }
        #else
        switch self {
        case .largeTitle: 34
        case .title: 28
        case .title2: 22
        case .title3: 20
        case .headline, .body: 17
        case .callout: 16
        case .subheadline: 15
        case .footnote: 13
        case .caption: 12
        case .caption2: 11
        @unknown default: 17
        }
        #endif
    }
}

extension Font {
    /// Manrope at a system text style's size, scaled with Dynamic Type.
    public static func app(_ style: Font.TextStyle) -> Font {
        AppFonts.register()
        return .custom(AppFonts.sans, size: style.defaultSize, relativeTo: style)
    }

    /// Geist Mono at a system text style's size, scaled with Dynamic Type:
    /// passwords, one-time codes and the Secret Key.
    public static func secret(_ style: Font.TextStyle) -> Font {
        AppFonts.register()
        return .custom(AppFonts.mono, size: style.defaultSize, relativeTo: style)
    }
}

extension View {
    /// `Text("…").textStyle(Tokens.Text.body)`
    public func textStyle(_ style: TypeStyle) -> some View {
        modifier(TokenText(style))
    }
}
#endif
