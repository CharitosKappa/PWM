import XCTest
@testable import PWMDesign
#if canImport(CoreText)
import CoreText
#endif

final class AppFontsTests: XCTestCase {
    func testBothFontFilesAreBundled() {
        XCTAssertEqual(AppFonts.urls.map(\.lastPathComponent).sorted(), ["GeistMono.ttf", "Manrope.ttf"])
    }

    #if canImport(CoreText)
    func testRegisteredFontsResolveByFamilyName() {
        AppFonts.register()
        for family in [AppFonts.sans, AppFonts.mono] {
            let font = CTFontCreateWithName(family as CFString, 12, nil)
            XCTAssertEqual(CTFontCopyFamilyName(font) as String, family)
        }
    }
    #endif
}
