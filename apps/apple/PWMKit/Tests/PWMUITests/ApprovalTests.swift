import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// Asking a signed-in device, and approving from one (D46 §5).
@MainActor
final class ApprovalTests: XCTestCase {
    var account: FakeAccount!

    override func setUp() async throws {
        account = FakeAccount()
    }

    func signIn() -> SignInModel {
        SignInModel(deviceName: "iPhone", account: { [unowned self] in self.account }, onScannedServer: { _ in }, onSignedIn: {})
    }

    func testAskingFillsInTheSecretKeyAndLeavesThePassword() async throws {
        let signIn = signIn()
        XCTAssertFalse(signIn.canAsk, "needs the email")
        signIn.email = " ada@example.com "
        XCTAssertTrue(signIn.canAsk)
        await signIn.askAnotherDevice()
        XCTAssertTrue(signIn.approved)
        XCTAssertEqual(signIn.secretKey, kitKey)
        XCTAssertNil(signIn.approvalCode, "the code goes once it's done")
        XCTAssertFalse(signIn.canSubmit, "the master password is still needed")
        signIn.password = strong
        XCTAssertTrue(signIn.canSubmit)
        let calls = await account.calls
        XCTAssertEqual(calls, ["ask ada@example.com"])
    }

    func testAnEndedRequestSaysSo() async throws {
        await account.setAsk(code: "111 222", result: .failure(PWMError(code: "approval_ended")))
        let signIn = signIn()
        signIn.email = "ada@example.com"
        await signIn.askAnotherDevice()
        XCTAssertFalse(signIn.approved)
        XCTAssertEqual(signIn.secretKey, "")
        XCTAssertEqual(signIn.error, PWMError(code: "approval_ended").message)
    }

    func entry(_ id: String) -> ApprovalEntry {
        ApprovalEntry(id: id, platform: "macos", created: Date(timeIntervalSince1970: 1), state: "pending", code: nil)
    }

    func testApprovingShowsTheCodeThenSendsTheKeyOnlyWhenTheyMatch() async throws {
        await account.setApprovals([entry("a"), entry("b")])
        let approvals = ApprovalsModel(account: account, pollEvery: .milliseconds(10))
        await approvals.load()
        XCTAssertEqual(approvals.requests.map(\.id), ["a", "b"])
        XCTAssertNil(approvals.code)

        await approvals.codesMatch()
        var calls = await account.calls
        XCTAssertFalse(calls.contains { $0.hasPrefix("complete") }, "nothing sent before a code")

        await approvals.approve(approvals.requests[0])
        await approvals.waitForCode()
        XCTAssertEqual(approvals.code, "654 321")
        await approvals.codesMatch()
        XCTAssertTrue(approvals.done)
        XCTAssertNil(approvals.approving)
        XCTAssertEqual(approvals.requests.map(\.id), ["b"])

        await approvals.deny("b")
        XCTAssertEqual(approvals.requests, [])
        calls = await account.calls
        XCTAssertEqual(calls, ["accept a", "complete a", "deny b"])
    }

    func testARequestThatEndsWhileApprovingSaysSo() async throws {
        await account.setApprovals([entry("a")])
        let approvals = ApprovalsModel(account: account, pollEvery: .milliseconds(10))
        await approvals.load()
        await approvals.approve(approvals.requests[0])
        await account.setApprovals([])
        await approvals.load()
        XCTAssertNil(approvals.approving)
        XCTAssertEqual(approvals.error, PWMError(code: "unknown_approval").message)
    }

    func testPendingRequestsAreCountedForTheBadge() async throws {
        await account.setSignedIn(email: "ada@example.com")
        await account.setApprovals([entry("a")])
        let app = AppModel(
            makeAccount: { [unowned self] _ in self.account },
            defaults: UserDefaults(suiteName: "pwm-approvals-\(UUID().uuidString)")!,
            pasteboard: FakePasteboard(),
            deviceName: "iPhone"
        )
        await app.start()
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        await app.willEnterForeground()
        XCTAssertEqual(app.settings?.pendingApprovals, 1)
    }
}
