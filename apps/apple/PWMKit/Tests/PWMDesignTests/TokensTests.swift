import XCTest
@testable import PWMDesign

final class TokensTests: XCTestCase {
    func testColorsHaveLightAndDarkValues() {
        XCTAssertEqual(Tokens.version, 1)
        XCTAssertEqual(Tokens.Color.bg.light.hex, "#FFFFFF")
        XCTAssertNotEqual(Tokens.Color.bg.light, Tokens.Color.bg.dark)
        XCTAssertEqual(Tokens.Color.accent.light.alpha, 1)
    }

    func testTextAndSpacing() {
        XCTAssertTrue(Tokens.Text.secret.monospaced)
        XCTAssertFalse(Tokens.Text.body.monospaced)
        XCTAssertEqual(Tokens.Text.bodyStrong.weight, 600)
        XCTAssertEqual(Tokens.Space.s4, 16)
        XCTAssertEqual(Tokens.Radius.md, 16)
    }
}
