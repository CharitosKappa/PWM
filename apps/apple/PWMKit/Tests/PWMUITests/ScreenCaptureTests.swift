import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// What the screens hide while the screen is recorded or shared, and after a
/// screenshot (D49), and the macOS window setting.
@MainActor
final class ScreenCaptureTests: XCTestCase {
    var account: FakeAccount!
    var pasteboard: FakePasteboard!
    var capture: ScreenCapture!

    override func setUp() async throws {
        account = FakeAccount()
        pasteboard = FakePasteboard()
        capture = ScreenCapture()
    }

    func detail() -> ItemDetailModel {
        ItemDetailModel(
            entry: Entry(vaultId: "v", item: loginItem("1", "Bank", username: "ada", password: "hunter2")),
            account: account, pasteboard: pasteboard, capture: capture
        )
    }

    func testRecordingHidesRevealedValuesAndTurnsRevealingOff() async {
        let model = detail()
        model.toggle("password")
        XCTAssertTrue(model.isRevealed("password"))
        XCTAssertFalse(model.hiddenByCapture)

        capture.setRecorded(true)
        XCTAssertTrue(model.hiddenByCapture)
        XCTAssertFalse(model.isRevealed("password"))
        model.toggle("password")
        XCTAssertFalse(model.isRevealed("password"), "nothing reveals while recording")
        model.copy("hunter2", field: "password")
        XCTAssertEqual(pasteboard.copied.map(\.0), ["hunter2"], "copying still works")

        capture.setRecorded(false)
        XCTAssertFalse(model.hiddenByCapture)
        XCTAssertFalse(model.isRevealed("password"), "it doesn't come back by itself")
        model.toggle("password")
        XCTAssertTrue(model.isRevealed("password"))
    }

    func testAScreenshotMasksWhatWasShown() async {
        let model = detail()
        capture.screenshotTaken()
        XCTAssertNil(model.notice, "nothing was shown, so no line")

        model.toggle("password")
        capture.screenshotTaken()
        XCTAssertFalse(model.isRevealed("password"))
        XCTAssertEqual(model.notice, ScreenCapture.screenshotLine)
        model.toggle("password")
        XCTAssertNil(model.notice, "gone at the next tap")
    }

    func testTheSecretKeyAndTheGeneratorFollowTheCapture() async {
        let key = SecretKeyModel(secretKey: Redacted(kitKey), account: account, pasteboard: pasteboard, capture: capture) {}
        let generator = GeneratorModel(pasteboard: pasteboard, capture: capture)
        XCTAssertFalse(key.hiddenByCapture)
        XCTAssertFalse(generator.hiddenByCapture)
        capture.setRecorded(true)
        XCTAssertTrue(key.hiddenByCapture)
        XCTAssertTrue(generator.hiddenByCapture)
    }

    func testListenersThatAreGoneAreDropped() async {
        var model: ItemDetailModel? = detail()
        model?.toggle("password")
        weak var gone = model
        model = nil
        XCTAssertNil(gone)
        capture.setRecorded(true)
        capture.screenshotTaken()
    }

    func testTheMacSettingIsKeptAndOnByDefault() async throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "pwm-capture-\(UUID().uuidString)"))
        let app = AppModel(makeAccount: { [unowned self] _ in self.account }, defaults: defaults, pasteboard: pasteboard, deviceName: "Mac")
        XCTAssertTrue(app.hidesFromCapture)
        XCTAssertFalse(app.hidesWindow, "nothing secret on the loading screen")
        let settings = SettingsModel(server: nil, app: app, account: account)
        XCTAssertTrue(settings.hidesFromCaptureOn)
        settings.setHidesFromCapture(false)
        XCTAssertFalse(settings.hidesFromCaptureOn)
        XCTAssertFalse(AppModel(makeAccount: { [unowned self] _ in self.account }, defaults: defaults, pasteboard: pasteboard, deviceName: "Mac").hidesFromCapture)
    }
}
