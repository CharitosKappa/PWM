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

    /// "Allow browser extensions to unlock with Touch ID" (D48 §5).
    func testTheBrowserTouchIDSwitch() async throws {
        let system = FakeBrowserTouchID()
        let app = AppModel(
            makeAccount: { [unowned self] _ in self.account }, pasteboard: pasteboard, deviceName: "Mac",
            browserTouchID: system
        )
        let settings = SettingsModel(server: nil, app: app, account: account)
        await settings.load()
        XCTAssertEqual(settings.browserTouchIDOn, false)
        settings.setBrowserTouchID(true)
        XCTAssertEqual(settings.browserTouchIDOn, true)
        system.fails = true
        settings.setBrowserTouchID(false)
        settings.setBrowserTouchID(true)
        XCTAssertEqual(settings.browserTouchIDOn, false)
        XCTAssertNotNil(settings.error)

        // Settings holds the app unowned: keep it alive for the test.
        let phone = AppModel(makeAccount: { [unowned self] _ in self.account }, pasteboard: pasteboard, deviceName: "iPhone")
        let elsewhere = SettingsModel(server: nil, app: phone, account: account)
        await elsewhere.load()
        XCTAssertNil(elsewhere.browserTouchIDOn, "no switch without the Mac helper")
    }

    /// "Sign Out…" in the Mac menu bar (D48 §2): Settings asks once.
    func testSignOutFromTheMenuAsksInSettings() async {
        let app = AppModel(makeAccount: { [unowned self] _ in self.account }, pasteboard: pasteboard, deviceName: "Mac")
        let settings = SettingsModel(server: nil, app: app, account: account)
        let before = app.settingsRequests
        app.askSignOut()
        XCTAssertEqual(app.settingsRequests, before + 1, "Settings is shown")
        XCTAssertTrue(settings.signOutAsked)
        settings.signOutQuestionShown()
        XCTAssertFalse(settings.signOutAsked, "asked once")
    }
}

final class FakeBrowserTouchID: BrowserTouchID, @unchecked Sendable {
    var on = false
    var fails = false

    var isOn: Bool { on }

    func turnOn() throws {
        if fails { throw PWMError(code: "storage") }
        on = true
    }

    func turnOff() { on = false }
}
