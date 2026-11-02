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
        self.style = style
        _size = ScaledMetric(wrappedValue: style.size, relativeTo: style.textStyle)
    }

    func body(content: Content) -> some View {
        content
            .font(.system(size: size, weight: style.fontWeight, design: style.monospaced ? .monospaced : .default))
            .lineSpacing(size * (style.lineHeight - 1))
    }
}

extension View {
    /// `Text("…").textStyle(Tokens.Text.body)`
    public func textStyle(_ style: TypeStyle) -> some View {
        modifier(TokenText(style))
    }
}
#endif
