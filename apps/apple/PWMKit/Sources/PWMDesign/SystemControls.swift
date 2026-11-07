// Manrope in the iOS system controls that SwiftUI's `.font` doesn't reach
// (D1): navigation bar titles and buttons, tab bar labels and segmented
// pickers. They follow Dynamic Type through UIFontMetrics, like the system
// styles they replace. Alerts, menus and the keyboard keep the system font;
// they can't be changed. On macOS, window titles and toolbars keep it too,
// as in every Mac app.
#if canImport(UIKit) && os(iOS)
import CoreText
import UIKit

public enum SystemControls {
    /// Sets the controls' appearance once, before the first bar is drawn:
    /// at launch in the app, and when the AutoFill extension's view loads.
    /// Later calls do nothing.
    @MainActor public static func useAppFonts() {
        guard !applied else { return }
        applied = true
        AppFonts.register()

        let title = font(size: 17, weight: 600, style: .headline)
        let largeTitle = font(size: 34, weight: 700, style: .largeTitle)
        let button = font(size: 17, weight: 400, style: .body)
        let done = font(size: 17, weight: 600, style: .body)
        func styled(_ appearance: UINavigationBarAppearance) -> UINavigationBarAppearance {
            appearance.titleTextAttributes = [.font: title]
            appearance.largeTitleTextAttributes = [.font: largeTitle]
            appearance.buttonAppearance.normal.titleTextAttributes = [.font: button]
            appearance.backButtonAppearance.normal.titleTextAttributes = [.font: button]
            appearance.doneButtonAppearance.normal.titleTextAttributes = [.font: done]
            return appearance
        }
        // The system's own backgrounds: blurred when content scrolls under
        // the bar, transparent at the top.
        let standard = UINavigationBarAppearance()
        standard.configureWithDefaultBackground()
        let edge = UINavigationBarAppearance()
        edge.configureWithTransparentBackground()
        let bar = UINavigationBar.appearance()
        bar.standardAppearance = styled(standard)
        bar.compactAppearance = styled(standard.copy())
        bar.scrollEdgeAppearance = styled(edge)
        bar.compactScrollEdgeAppearance = styled(edge.copy())

        let label = font(size: 12, weight: 500, style: .caption1)
        func tabs(_ appearance: UITabBarAppearance) -> UITabBarAppearance {
            for item in [appearance.stackedLayoutAppearance, appearance.inlineLayoutAppearance, appearance.compactInlineLayoutAppearance] {
                item.normal.titleTextAttributes = [.font: label]
                item.selected.titleTextAttributes = [.font: label]
            }
            return appearance
        }
        let tabStandard = UITabBarAppearance()
        tabStandard.configureWithDefaultBackground()
        let tabEdge = UITabBarAppearance()
        tabEdge.configureWithTransparentBackground()
        UITabBar.appearance().standardAppearance = tabs(tabStandard)
        UITabBar.appearance().scrollEdgeAppearance = tabs(tabEdge)

        let segments = UISegmentedControl.appearance()
        segments.setTitleTextAttributes([.font: font(size: 13, weight: 500, style: .subheadline)], for: .normal)
        segments.setTitleTextAttributes([.font: font(size: 13, weight: 600, style: .subheadline)], for: .selected)
    }

    @MainActor private static var applied = false

    /// The variable font's weight axis, `wght`.
    private static let weightAxis = 0x7767_6874

    /// Manrope at a weight (100 to 900) and the system style's default size,
    /// scaled with Dynamic Type. Where Manrope is missing, the system font.
    static func font(size: CGFloat, weight: Int, style: UIFont.TextStyle) -> UIFont {
        let variation = UIFontDescriptor.AttributeName(rawValue: kCTFontVariationAttribute as String)
        let descriptor = UIFontDescriptor(fontAttributes: [
            .family: AppFonts.sans,
            variation: [NSNumber(value: weightAxis): NSNumber(value: weight)],
        ])
        let manrope = UIFont(descriptor: descriptor, size: size)
        let base = manrope.familyName == AppFonts.sans ? manrope : UIFont.systemFont(ofSize: size, weight: systemWeight(weight))
        return UIFontMetrics(forTextStyle: style).scaledFont(for: base)
    }

    private static func systemWeight(_ weight: Int) -> UIFont.Weight {
        switch weight {
        case ..<450: .regular
        case ..<550: .medium
        case ..<650: .semibold
        default: .bold
        }
    }
}
#endif
