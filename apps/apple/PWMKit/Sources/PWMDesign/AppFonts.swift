// The app fonts (D1): Manrope for text, Geist Mono for passwords, codes and
// the Secret Key. The files in Fonts/ are copies of design/fonts made by
// scripts/fonts.mjs (`just check` fails when they differ); their licence and
// notice ship in the third-party notices. They are registered for this
// process only, before the first text that uses them.
import Foundation
#if canImport(CoreText)
import CoreText
#endif

public enum AppFonts {
    /// The family names, the first entries of `font.family` in design/tokens.json.
    public static let sans = "Manrope"
    public static let mono = "Geist Mono"

    /// The bundled font files.
    public static let files = ["Manrope.ttf", "GeistMono.ttf"]

    /// The bundled files' locations; a file that is missing is left out.
    public static var urls: [URL] {
        files.compactMap { Bundle.module.url(forResource: $0, withExtension: nil, subdirectory: "Fonts") }
    }

    /// Registers the fonts once; later calls do nothing. Where the fonts are
    /// missing, text falls back to the system font.
    public static func register() {
        _ = registered
    }

    private static let registered: Bool = {
        #if canImport(CoreText)
        let list = urls
        guard !list.isEmpty else { return false }
        CTFontManagerRegisterFontURLs(list as CFArray, .process, true, nil)
        return true
        #else
        return false
        #endif
    }()
}
