import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

@MainActor
final class OnboardingTests: XCTestCase {
    var account: FakeAccount!
    var servers: [URL] = []
    var pasteboard: FakePasteboard!
    var defaults: UserDefaults!

    override func setUp() async throws {
        account = FakeAccount()
        servers = []
        pasteboard = FakePasteboard()
        defaults = UserDefaults(suiteName: "pwm-tests-\(UUID().uuidString)")
    }

    func app() -> AppModel {
        AppModel(
            makeAccount: { [unowned self] server in
                self.servers.append(server)
                return self.account
            },
            defaults: defaults,
            pasteboard: pasteboard,
            deviceName: "iPhone"
        )
    }

    func testAFreshInstallStartsAtWelcomeOnTheHostedServer() async {
        let app = app()
        await app.start()
        XCTAssertEqual(app.screen, .welcome)
        XCTAssertFalse(app.usesOtherServer)
        XCTAssertEqual(servers, [Server.hosted])
    }

    func testASignedInDeviceStartsAtUnlockOnItsServer() async {
        defaults.set("https://vault.example.org", forKey: AppModel.serverKey)
        await account.setSignedIn(email: "ada@example.com")
        let app = app()
        await app.start()
        XCTAssertEqual(app.screen, .unlock)
        XCTAssertEqual(app.unlock?.email, "ada@example.com")
        XCTAssertTrue(app.usesOtherServer)
        XCTAssertEqual(app.otherServer, "https://vault.example.org")
        XCTAssertEqual(servers, [URL(string: "https://vault.example.org")!])
    }

    func testCreatingAnAccountNeedsAStrongMatchingPassword() async throws {
        let app = app()
        await app.start()
        app.showCreateAccount()
        let create = try XCTUnwrap(app.createAccount)
        create.email = " ada@example.com "
        create.password = "password"
        await create.updateStrength()
        create.confirm = "password"
        XCTAssertFalse(create.strongEnough)
        XCTAssertFalse(create.canSubmit)

        create.password = strong
        await create.updateStrength()
        XCTAssertTrue(create.strongEnough)
        XCTAssertFalse(create.canSubmit, "the confirmation still differs")
        create.confirm = strong
        XCTAssertTrue(create.canSubmit)

        await create.submit()
        let calls = await account.calls
        XCTAssertEqual(calls, ["signUp ada@example.com iPhone"])
        XCTAssertEqual(app.screen, .secretKey)
        XCTAssertEqual(create.password, "", "cleared once sent to the core")
        XCTAssertEqual(app.secretKey?.secretKey.value, kitKey)
    }

    func testASignUpErrorStaysOnTheScreen() async throws {
        let app = app()
        await app.start()
        app.showCreateAccount()
        let create = try XCTUnwrap(app.createAccount)
        create.email = "ada@example.com"
        create.password = strong
        create.confirm = strong
        await create.updateStrength()
        await account.fail(with: "account_exists")
        await create.submit()
        XCTAssertEqual(app.screen, .createAccount)
        XCTAssertEqual(create.error, "There is already an account with this email.")
    }

    func testTheSecretKeyScreenNeedsTheKitSavedOrTheKeyCopied() async throws {
        let app = try await signedUp()
        let key = try XCTUnwrap(app.secretKey)
        XCTAssertFalse(key.canContinue)
        key.done()
        XCTAssertEqual(app.screen, .secretKey, "I saved it is disabled")

        await key.prepareKit(paper: .letter)
        XCTAssertEqual(key.kit?.value, Data("%PDF-fake".utf8))
        key.copy()
        XCTAssertEqual(pasteboard.copied.map(\.0), [kitKey])
        XCTAssertEqual(pasteboard.copied.map(\.1), [30])
        XCTAssertTrue(key.canContinue)

        key.done()
        XCTAssertEqual(app.screen, .vault)
        XCTAssertNil(key.kit, "the PDF is dropped when the screen goes")
        let calls = await account.calls
        XCTAssertTrue(calls.contains("kit letter"))
    }

    func testSharingTheKitCountsAsSaved() async throws {
        let app = try await signedUp()
        let key = try XCTUnwrap(app.secretKey)
        key.didShareKit()
        XCTAssertTrue(key.canContinue)
    }

    func testAScannedKitFillsInEverythingButThePassword() async throws {
        let app = app()
        await app.start()
        app.showSignIn()
        let signIn = try XCTUnwrap(app.signIn)
        XCTAssertTrue(signIn.scanned("pwm-kit:1?email=ada%40example.com&secret-key=\(kitKey)&server=https%3A%2F%2Fvault.example.org"))
        XCTAssertEqual(signIn.email, "ada@example.com")
        XCTAssertEqual(signIn.secretKey, kitKey)
        XCTAssertEqual(signIn.password, "")
        XCTAssertTrue(app.usesOtherServer)
        XCTAssertEqual(app.otherServer, "https://vault.example.org")
        XCTAssertFalse(signIn.canSubmit)

        signIn.password = strong
        await signIn.submit()
        XCTAssertEqual(app.screen, .vault)
        XCTAssertEqual(servers.last, URL(string: "https://vault.example.org")!)
        let calls = await account.calls
        XCTAssertEqual(calls, ["logIn ada@example.com \(strong) \(kitKey)"])
        XCTAssertEqual(signIn.password, "")
        XCTAssertEqual(signIn.secretKey, "")
    }

    func testOtherQRCodesAreRefused() async throws {
        let app = app()
        await app.start()
        app.showSignIn()
        let signIn = try XCTUnwrap(app.signIn)
        signIn.email = "ada@example.com"
        XCTAssertFalse(signIn.scanned("https://example.com"))
        XCTAssertEqual(signIn.error, "That's not an Emergency Kit code.")
        XCTAssertEqual(signIn.email, "ada@example.com")
        XCTAssertFalse(signIn.scanned("pwm-kit:2?email=x"))
        XCTAssertEqual(signIn.error, PWMError(code: "kit_too_new").message)
    }

    func testABadServerAddressIsRefusedAtSignIn() async throws {
        let app = app()
        await app.start()
        app.usesOtherServer = true
        app.otherServer = "http://vault.example.org"
        app.showSignIn()
        let signIn = try XCTUnwrap(app.signIn)
        signIn.email = "ada@example.com"
        signIn.password = strong
        signIn.secretKey = kitKey
        await signIn.submit()
        XCTAssertEqual(signIn.error, PWMError(code: "invalid_server").message)
        XCTAssertEqual(app.screen, .signIn)
    }

    func testServerAddresses() throws {
        XCTAssertEqual(try AppModel.serverAddress(" https://vault.example.org/ "), URL(string: "https://vault.example.org")!)
        XCTAssertEqual(try AppModel.serverAddress("http://localhost:8080"), URL(string: "http://localhost:8080")!)
        XCTAssertEqual(try AppModel.serverAddress("http://127.0.0.1:8080"), URL(string: "http://127.0.0.1:8080")!)
        for bad in ["", "vault.example.org", "http://vault.example.org", "ftp://vault.example.org", "https://"] {
            XCTAssertThrowsError(try AppModel.serverAddress(bad), bad)
        }
    }

    func testUnlockLockAndSignOut() async throws {
        await account.setSignedIn(email: "ada@example.com")
        let app = app()
        await app.start()
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = "wrong"
        await unlock.submit()
        XCTAssertEqual(unlock.error, "Wrong master password.")
        XCTAssertEqual(unlock.password, "")
        XCTAssertEqual(app.screen, .unlock)

        unlock.password = strong
        await unlock.submit()
        XCTAssertEqual(app.screen, .vault)
        XCTAssertNotNil(app.vault)
        XCTAssertNotNil(app.generator)
        let settings = try XCTUnwrap(app.settings)
        await settings.load()
        XCTAssertEqual(settings.email, "ada@example.com")

        await settings.lockNow()
        XCTAssertEqual(app.screen, .unlock)
        XCTAssertNil(app.vault, "the vault's items go with the lock")
        XCTAssertNil(app.settings)
        await app.unlock?.signOut()
        XCTAssertEqual(app.screen, .welcome)
        let calls = await account.calls
        XCTAssertEqual(calls, ["unlock", "lock", "signOut"])
    }

    private func signedUp() async throws -> AppModel {
        let app = app()
        await app.start()
        app.showCreateAccount()
        let create = try XCTUnwrap(app.createAccount)
        create.email = "ada@example.com"
        create.password = strong
        create.confirm = strong
        await create.updateStrength()
        await create.submit()
        XCTAssertEqual(app.screen, .secretKey)
        return app
    }
}
