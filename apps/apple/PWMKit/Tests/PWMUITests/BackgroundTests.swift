import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// Background sync, the push token, "Sign in again to sync" and the
/// approval notice switch (D50), over a fake account.
@MainActor
final class BackgroundTests: XCTestCase {
    var account: FakeAccount!
    var defaults: UserDefaults!
    var notices: FakeApprovalNotices!

    override func setUp() async throws {
        account = FakeAccount()
        defaults = UserDefaults(suiteName: "pwm-background-\(UUID().uuidString)")
        notices = FakeApprovalNotices()
        await account.setSignedIn(email: "ada@example.com")
    }

    func app() -> AppModel {
        AppModel(
            makeAccount: { [unowned self] _ in self.account },
            defaults: defaults,
            pasteboard: FakePasteboard(),
            deviceName: "iPhone",
            approvalNotices: notices
        )
    }

    func unlocked(_ app: AppModel) async throws {
        await app.start()
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        XCTAssertEqual(app.screen, .vault)
    }

    func testABackgroundSyncRunsLockedAndReadsTheVaultAgain() async throws {
        let app = app()
        await app.start()
        XCTAssertEqual(app.screen, .unlock)
        await account.setSyncResult(.synced(pulled: 2, pushed: 0, conflicts: 0, failed: 0))
        let locked = await app.backgroundSync()
        XCTAssertEqual(locked, .newData)
        XCTAssertEqual(app.screen, .unlock, "it stays locked")

        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        await account.put(loginItem("1", "Bank"))
        await account.setSyncResult(.synced(pulled: 0, pushed: 0, conflicts: 0, failed: 0))
        let unlockedResult = await app.backgroundSync()
        XCTAssertEqual(unlockedResult, .noData)
        XCTAssertEqual(app.vault?.entries.map(\.item.title), ["Bank"])

        await account.setSyncResult(.retry(afterMs: 1000))
        let retried = await app.backgroundSync()
        XCTAssertEqual(retried, .failed)
        let calls = await account.calls
        XCTAssertEqual(calls.filter { $0 == "backgroundSync" }.count, 3)
    }

    func testNothingRunsWithoutAnAccount() async {
        await account.signOut()
        let app = app()
        await app.start()
        let result = await app.backgroundSync()
        XCTAssertEqual(result, .noData)
        let calls = await account.calls
        XCTAssertFalse(calls.contains("backgroundSync"))
    }

    func testARevokedDeviceFoundInTheBackgroundSignsOut() async {
        let app = app()
        await app.start()
        await account.setSyncResult(.deviceRevoked)
        _ = await app.backgroundSync()
        XCTAssertEqual(app.screen, .welcome)
        let calls = await account.calls
        XCTAssertEqual(calls.last, "signOut")
    }

    func testARefusedSessionAsksToSignInAgainKeepingTheVault() async throws {
        let app = app()
        try await unlocked(app)
        let vault = try XCTUnwrap(app.vault)
        let again = try XCTUnwrap(vault.signInAgain)
        XCTAssertFalse(again.needed)

        await account.setSyncResult(.signedOut)
        await vault.refresh()
        XCTAssertTrue(again.needed)
        XCTAssertNil(vault.status, "the line replaces the notice")
        XCTAssertEqual(app.screen, .vault, "the vault still shows")

        // Kept across launches.
        let next = self.app()
        try await unlocked(next)
        XCTAssertEqual(next.vault?.signInAgain?.needed, true)

        XCTAssertFalse(again.canSignIn)
        again.password = "wrong"
        let wrong = await again.signIn()
        XCTAssertFalse(wrong)
        XCTAssertEqual(again.error, "Wrong email, master password or Secret Key.")
        XCTAssertTrue(again.needed)

        again.password = strong
        let signedIn = await again.signIn()
        XCTAssertTrue(signedIn)
        XCTAssertFalse(again.needed)
        XCTAssertEqual(again.password, "", "the password is not kept")
        XCTAssertFalse(defaults.bool(forKey: SignInAgainModel.key))
        let calls = await account.calls
        XCTAssertTrue(calls.contains("signInAgain"))
        XCTAssertFalse(calls.contains("signOut"), "nothing on the device was dropped")
    }

    func testARefusalSeenWhileLockedShowsAfterUnlock() async throws {
        let app = app()
        await app.start()
        await account.setSyncResult(.signedOut)
        _ = await app.backgroundSync()
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        XCTAssertEqual(app.vault?.signInAgain?.needed, true)
    }

    func testThePushTokenGoesUpWhenItChangesAndAfterASignIn() async throws {
        let app = app()
        await app.start()
        await app.pushTokenReceived(Data([0xab, 0xcd]), sandbox: true)
        await app.pushTokenReceived(Data([0xab, 0xcd]), sandbox: true)
        await app.pushTokenReceived(Data([0x01]), sandbox: false)
        var calls = await account.calls
        XCTAssertEqual(calls.filter { $0.hasPrefix("push") }, ["push abcd sandbox", "push 01 production"])

        // A failure is tried again at the next launch.
        await account.setPushFails(true)
        await app.pushTokenReceived(Data([0x02]), sandbox: false)
        await account.setPushFails(false)
        await self.app().start()
        calls = await account.calls
        XCTAssertEqual(calls.filter { $0.hasPrefix("push") }.last, "push 02 production")

        // Signing out removes it from the server first.
        try await unlocked(app)
        await app.settings?.signOut()
        calls = await account.calls
        XCTAssertEqual(Array(calls.suffix(2)), ["push none production", "signOut"])
    }

    func testNoTokenMeansNoPushCalls() async throws {
        let app = app()
        try await unlocked(app)
        await app.settings?.signOut()
        let calls = await account.calls
        XCTAssertFalse(calls.contains { $0.hasPrefix("push") })
    }

    func testTheApprovalNoticeSwitchAsksTheSystemOnlyWhenTurnedOn() async throws {
        let app = app()
        try await unlocked(app)
        let settings = try XCTUnwrap(app.settings)
        await settings.load()
        XCTAssertFalse(settings.approvalNoticesOn)
        XCTAssertEqual(notices.asked, 0, "no prompt at launch")

        let opened = await settings.setApprovalNotices(true)
        XCTAssertNil(opened)
        XCTAssertTrue(settings.approvalNoticesOn)
        XCTAssertEqual(notices.asked, 1)

        // Only the system's settings turn it off.
        let off = await settings.setApprovalNotices(false)
        XCTAssertEqual(off, notificationSettingsURL)
        XCTAssertTrue(settings.approvalNoticesOn)

        // Refused before: the system's settings, and still off.
        notices.allowed = false
        notices.refused = true
        let refused = await settings.setApprovalNotices(true)
        XCTAssertEqual(refused, notificationSettingsURL)
        XCTAssertFalse(settings.approvalNoticesOn)
    }
}

final class FakeApprovalNotices: ApprovalNotices, @unchecked Sendable {
    var allowed = false
    var refused = false
    var asked = 0

    func isOn() async -> Bool { allowed }

    func turnOn() async -> Bool {
        if refused { return false }
        asked += 1
        allowed = true
        return true
    }
}
