import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// Face ID / Touch ID on the unlock screen and in Settings (D45 §3–4).
@MainActor
final class BiometricTests: XCTestCase {
    var account: FakeAccount!
    var defaults: UserDefaults!

    override func setUp() async throws {
        account = FakeAccount()
        defaults = UserDefaults(suiteName: "pwm-biometric-\(UUID().uuidString)")
        await account.setSignedIn(email: "ada@example.com")
    }

    func app() async -> AppModel {
        let app = AppModel(
            makeAccount: { [unowned self] _ in self.account },
            defaults: defaults,
            pasteboard: FakePasteboard(),
            deviceName: "iPhone"
        )
        await app.start()
        await app.unlock?.load()
        return app
    }

    func testTheOfferComesOnceAfterAPasswordUnlock() async throws {
        var app = await app()
        var unlock = try XCTUnwrap(app.unlock)
        XCTAssertNil(unlock.biometricKind, "off until turned on")
        unlock.password = strong
        await unlock.submit()
        XCTAssertEqual(unlock.offer, .faceID)
        XCTAssertEqual(app.screen, .unlock, "the offer shows before the vault")
        await unlock.acceptOffer()
        XCTAssertEqual(app.screen, .vault)
        let state = await account.biometric
        XCTAssertEqual(state, .on(.faceID))

        // Turned off later: the offer does not come back.
        await account.disableBiometricUnlock()
        app = await self.app()
        unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        XCTAssertNil(unlock.offer)
        XCTAssertEqual(app.screen, .vault)
    }

    func testNotNowIsNeverAskedAgain() async throws {
        var app = await app()
        var unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        unlock.declineOffer()
        XCTAssertEqual(app.screen, .vault)
        let calls = await account.calls
        XCTAssertFalse(calls.contains("enable biometric"))

        app = await self.app()
        unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        XCTAssertNil(unlock.offer)
    }

    func testNoOfferWithoutTheHardware() async throws {
        await account.setBiometric(.unavailable(reason: "No Secure Enclave."))
        let app = await app()
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        XCTAssertNil(unlock.offer)
        XCTAssertEqual(app.screen, .vault)
    }

    func testFaceIDUnlocksAndACancelKeepsThePasswordField() async throws {
        await account.setBiometric(.on(.faceID), failure: "biometric_cancelled")
        var app = await app()
        var unlock = try XCTUnwrap(app.unlock)
        XCTAssertEqual(unlock.biometricKind, .faceID)
        await unlock.unlockWithBiometrics()
        XCTAssertEqual(app.screen, .unlock)
        XCTAssertNil(unlock.error, "a cancel is not an error")

        await account.setBiometric(.on(.faceID))
        app = await self.app()
        unlock = try XCTUnwrap(app.unlock)
        await unlock.unlockWithBiometrics()
        XCTAssertEqual(app.screen, .vault)
    }

    func testAChangedFaceTurnsItOffUntilThePasswordTurnsItBackOn() async throws {
        await account.setBiometric(.on(.faceID), failure: "biometric_changed")
        let app = await app()
        let unlock = try XCTUnwrap(app.unlock)
        await unlock.unlockWithBiometrics()
        XCTAssertEqual(unlock.error, PWMError(code: "biometric_changed").message)
        XCTAssertNil(unlock.biometricKind)

        await account.setBiometric(.off(.faceID))
        unlock.password = strong
        await unlock.submit()
        XCTAssertNil(unlock.offer, "turned back on, not offered")
        XCTAssertEqual(app.screen, .vault)
        let state = await account.biometric
        XCTAssertEqual(state, .on(.faceID))
    }

    func testTheMasterPasswordWhenDue() async throws {
        await account.setBiometric(.on(.touchID), failure: "master_password_due")
        let app = await app()
        let unlock = try XCTUnwrap(app.unlock)
        XCTAssertEqual(unlock.biometricKind, .touchID)
        await unlock.unlockWithBiometrics()
        XCTAssertEqual(unlock.error, PWMError(code: "master_password_due").message)
        XCTAssertEqual(app.screen, .unlock)
    }

    func testTheSettingsSwitch() async throws {
        defaults.set(true, forKey: UnlockModel.offeredKey)
        let app = await app()
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        let settings = try XCTUnwrap(app.settings)
        await settings.load()
        XCTAssertEqual(settings.biometric, .off(.faceID))
        await settings.setBiometric(true)
        XCTAssertEqual(settings.biometric, .on(.faceID))
        await settings.setBiometric(false)
        XCTAssertEqual(settings.biometric, .off(.faceID))
        let calls = await account.calls
        XCTAssertEqual(calls.filter { $0.contains("biometric") }, ["enable biometric", "disable biometric"])
    }
}
