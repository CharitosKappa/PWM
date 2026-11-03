import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// The AutoFill extension's model (D47 §3-6) and the Settings card and
/// switch (§5, §7), over fakes.
@MainActor
final class AutoFillTests: XCTestCase {
    var account: FakeAutoFillAccount!
    var pasteboard: FakePasteboard!
    var results: [AutoFillResult] = []

    override func setUp() async throws {
        account = FakeAutoFillAccount()
        pasteboard = FakePasteboard()
        results = []
    }

    func model(_ request: AutoFillRequest) -> AutoFillModel {
        AutoFillModel(request: request, account: account, pasteboard: pasteboard) { [unowned self] in
            self.results.append($0)
        }
    }

    let shop = FillChoice(vaultId: "v", itemId: "1", title: "Shop", username: "ada")
    let bank = FillChoice(vaultId: "v", itemId: "2", title: "Bank", username: "ada@bank.example")
    let mail = FillChoice(vaultId: "v", itemId: "3", title: "Mail", username: "bob")

    // MARK: Unlocking

    func testNeverAsksForTheMasterPassword() async {
        await account.set(signedIn: false)
        var m = model(.logins(services: ["example.com"]))
        await m.start()
        XCTAssertEqual(m.screen, .blocked("Open PWM and sign in to fill here."))

        await account.set(signedIn: true, biometric: .off(.faceID))
        m = model(.logins(services: ["example.com"]))
        await m.start()
        XCTAssertEqual(m.screen, .blocked("Turn on Face ID in PWM to fill here."))

        await account.set(signedIn: true, biometric: .unavailable(reason: "no hardware"))
        m = model(.logins(services: ["example.com"]))
        await m.start()
        XCTAssertEqual(m.screen, .blocked("Turn on Face ID or Touch ID in PWM to fill here."))

        for code in ["master_password_due", "biometric_changed"] {
            await account.set(signedIn: true, biometric: .on(.faceID), failure: code)
            m = model(.logins(services: ["example.com"]))
            await m.start()
            XCTAssertEqual(m.screen, .blocked("Open PWM and enter your master password."), code)
        }
        XCTAssertTrue(results.isEmpty, "the user closes it")
        let unlocks = await account.calls.filter { $0 == "unlock" }
        XCTAssertTrue(unlocks.isEmpty)
    }

    func testCancellingFaceIDHandsTheRequestBack() async {
        await account.set(signedIn: true, biometric: .on(.faceID), failure: "biometric_cancelled")
        let m = model(.logins(services: ["example.com"]))
        await m.start()
        XCTAssertEqual(results, [.cancelled])
        let calls = await account.calls
        XCTAssertEqual(calls.last, "lock")
    }

    // MARK: Logins

    func testListsMatchesThenTheRestAndFillsOnATap() async {
        await account.set(signedIn: true, biometric: .on(.faceID))
        await account.setLogins(matches: [shop], all: [bank, mail, shop])
        await account.setCode("123456")
        let m = model(.logins(services: ["shop.example.com", "https://shop.example.com/login"]))
        await m.start()
        XCTAssertEqual(m.screen, .logins)
        XCTAssertEqual(m.matches, [shop])
        XCTAssertEqual(m.others, [bank, mail], "matches are not listed twice")
        let asked = await account.services
        XCTAssertEqual(asked, ["shop.example.com", "https://shop.example.com/login"])

        m.query = "BOB"
        XCTAssertEqual(m.others, [mail], "search covers usernames, any case")
        m.query = "ban"
        XCTAssertEqual(m.others, [bank])
        XCTAssertTrue(results.isEmpty, "nothing fills before a tap")

        await m.fill(bank)
        XCTAssertEqual(results, [.login(username: "ada@bank.example", password: Redacted("password of 2"))])
        XCTAssertEqual(pasteboard.copied.map(\.0), ["123456"])
        XCTAssertEqual(pasteboard.copied.map(\.1), [30], "the code expires after 30 seconds")
        let calls = await account.calls
        XCTAssertEqual(calls, ["unlock", "fill 2", "lock"], "locked once it answered")

        await m.cancel()
        XCTAssertEqual(results.count, 1, "answers once")
    }

    func testAChosenSuggestionFillsAfterFaceID() async {
        await account.set(signedIn: true, biometric: .on(.faceID))
        var m = model(.login(record: "v/1"))
        await m.start()
        XCTAssertEqual(results, [.login(username: "ada", password: Redacted("password of 1"))])
        XCTAssertTrue(pasteboard.copied.isEmpty, "no code, nothing copied")

        results = []
        m = model(.login(record: "v/gone"))
        await m.start()
        XCTAssertEqual(results, [.notFound], "iOS drops a suggestion for a deleted item")

        results = []
        m = model(.login(record: "nonsense"))
        await m.start()
        XCTAssertEqual(results, [.notFound])
    }

    func testCancelHandsTheRequestBack() async {
        await account.set(signedIn: true, biometric: .on(.faceID))
        await account.setLogins(matches: [], all: [shop])
        let m = model(.logins(services: []))
        await m.start()
        await m.cancel()
        XCTAssertEqual(results, [.cancelled])
    }

    // MARK: Passkeys

    func testSignsWithTheChosenPasskey() async {
        await account.set(signedIn: true, biometric: .on(.faceID))
        let ada = PasskeyChoice(vaultId: "v", itemId: "1", credentialId: Data([1]), userName: "ada")
        let bob = PasskeyChoice(vaultId: "v", itemId: "2", credentialId: Data([2]), userName: "bob")
        await account.setPasskeys([ada, bob])
        let hash = Data(repeating: 9, count: 32)
        let m = model(.passkeys(rpId: "example.com", clientDataHash: hash, allowed: []))
        await m.start()
        XCTAssertEqual(m.screen, .passkeys)
        XCTAssertEqual(m.passkeys, [ada, bob])
        XCTAssertTrue(results.isEmpty)
        await m.choose(bob)
        XCTAssertEqual(results, [.passkey(FakeAutoFillAccount.signature(bob), rpId: "example.com", clientDataHash: hash)])
        let calls = await account.calls
        XCTAssertEqual(calls, ["unlock", "sign 2 example.com", "lock"])
    }

    func testAChosenPasskeySuggestionSignsAtOnce() async {
        await account.set(signedIn: true, biometric: .on(.faceID))
        let ada = PasskeyChoice(vaultId: "v", itemId: "1", credentialId: Data([1]), userName: "ada")
        await account.setPasskeys([ada])
        let hash = Data(repeating: 9, count: 32)
        var m = model(.passkey(record: "v/1", credentialId: Data([1]), rpId: "example.com", clientDataHash: hash))
        await m.start()
        XCTAssertEqual(results, [.passkey(FakeAutoFillAccount.signature(ada), rpId: "example.com", clientDataHash: hash)])

        results = []
        m = model(.passkey(record: "v/1", credentialId: Data([7]), rpId: "example.com", clientDataHash: hash))
        await m.start()
        XCTAssertEqual(results, [.notFound])
    }

    func testSavesANewPasskeyAfterConfirming() async {
        await account.set(signedIn: true, biometric: .on(.faceID))
        let hash = Data(repeating: 3, count: 32)
        let m = model(.newPasskey(rpId: "example.com", userName: "ada", userHandle: Data([5]), clientDataHash: hash))
        await m.start()
        XCTAssertEqual(m.screen, .newPasskey)
        XCTAssertEqual(m.newPasskeySite?.rpId, "example.com")
        XCTAssertTrue(results.isEmpty, "nothing is made before Save")
        await m.saveNewPasskey()
        XCTAssertEqual(results, [.newPasskey(FakeAutoFillAccount.made, rpId: "example.com", clientDataHash: hash)])
        let calls = await account.calls
        XCTAssertEqual(calls, ["unlock", "register example.com ada", "lock"])
    }

    func testAFailedSaveStaysOnScreen() async {
        await account.set(signedIn: true, biometric: .on(.faceID))
        let m = model(.newPasskey(rpId: "co.uk", userName: "ada", userHandle: Data([5]), clientDataHash: Data()))
        await m.start()
        await account.failRegister("invalid_rp_id")
        await m.saveNewPasskey()
        XCTAssertTrue(results.isEmpty)
        XCTAssertNotNil(m.error)
    }

    // MARK: Settings

    func testTheCardShowsUntilTheProviderIsOnAndTheSwitchIsKept() async {
        let fakeAccount = FakeAccount()
        let system = FakeAutoFillSystem()
        let app = AppModel(
            makeAccount: { _ in fakeAccount }, pasteboard: pasteboard, deviceName: "iPhone", autofill: system
        )
        let settings = SettingsModel(server: nil, app: app, account: fakeAccount)
        await settings.load()
        XCTAssertEqual(settings.autofillOn, false)
        XCTAssertTrue(settings.suggestLogins, "on by default")
        await settings.openAutoFillSettings()
        XCTAssertEqual(system.opened, 1)
        system.on = true
        await settings.loadAutoFill()
        XCTAssertEqual(settings.autofillOn, true)
        var calls = await fakeAccount.calls
        XCTAssertEqual(calls.last, "suggestions on", "sent again once iOS takes them")

        await settings.setSuggestLogins(false)
        XCTAssertFalse(settings.suggestLogins)
        calls = await fakeAccount.calls
        XCTAssertEqual(calls.last, "suggestions off")
    }
}

/// The extension's account: records calls; every password is
/// "password of <id>".
actor FakeAutoFillAccount: AutoFillAccount {
    var signedIn = true
    var biometric: BiometricState = .on(.faceID)
    var failure: PWMError?
    var registerFailure: PWMError?
    var matches: [FillChoice] = []
    var all: [FillChoice] = [
        FillChoice(vaultId: "v", itemId: "1", title: "Shop", username: "ada"),
    ]
    var passkeys: [PasskeyChoice] = []
    var code: String?
    var services: [String] = []
    var calls: [String] = []

    static let made = NewPasskey(credentialId: Data([4, 2]), attestationObject: Data([0xA3]))

    static func signature(_ choice: PasskeyChoice) -> PasskeySignature {
        PasskeySignature(
            credentialId: choice.credentialId, userHandle: Data(choice.userName.utf8),
            authenticatorData: Data(repeating: 1, count: 37), signature: Data([0x30])
        )
    }

    func set(signedIn: Bool, biometric: BiometricState = .on(.faceID), failure: String? = nil) {
        self.signedIn = signedIn
        self.biometric = biometric
        self.failure = failure.map(PWMError.init(code:))
    }

    func setLogins(matches: [FillChoice], all: [FillChoice]) {
        self.matches = matches
        self.all = all
    }

    func setPasskeys(_ list: [PasskeyChoice]) { passkeys = list }
    func setCode(_ value: String?) { code = value }
    func failRegister(_ code: String) { registerFailure = PWMError(code: code) }

    func isSignedIn() -> Bool { signedIn }
    func biometricState() -> BiometricState { biometric }

    func unlockWithBiometrics(reason: String) throws {
        if let failure { throw failure }
        calls.append("unlock")
    }

    func lock() { calls.append("lock") }

    func fillChoices(for services: [String]) -> [FillChoice] {
        self.services = services
        return matches
    }

    func allFillChoices() -> [FillChoice] { all }

    func fillCredential(_ itemId: String, in vaultId: String) throws -> FillCredential {
        guard let choice = all.first(where: { $0.itemId == itemId && $0.vaultId == vaultId }) else {
            throw PWMError(code: "unknown_item")
        }
        calls.append("fill \(itemId)")
        return FillCredential(username: choice.username, password: Redacted("password of \(itemId)"), hasCode: code != nil)
    }

    func currentCode(_ itemId: String, in vaultId: String) -> (code: Redacted<String>, secondsLeft: UInt64)? {
        code.map { (code: Redacted($0), secondsLeft: 20) }
    }

    func passkeyChoices(rpId: String, allowed: [Data]) -> [PasskeyChoice] {
        passkeys.filter { allowed.isEmpty || allowed.contains($0.credentialId) }
    }

    func registerPasskey(rpId: String, userName: String, userHandle: Data) throws -> NewPasskey {
        if let registerFailure { throw registerFailure }
        calls.append("register \(rpId) \(userName)")
        return Self.made
    }

    func signWithPasskey(_ choice: PasskeyChoice, rpId: String, clientDataHash: Data) -> PasskeySignature {
        calls.append("sign \(choice.itemId) \(rpId)")
        return Self.signature(choice)
    }
}

final class FakeAutoFillSystem: AutoFillSystem, @unchecked Sendable {
    var on = false
    var opened = 0

    func isProviderOn() async -> Bool { on }
    func openSettings() async { opened += 1 }
}
