import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// Deleting the account from Settings (D74 §7): the plan first when there
/// is one, then the master password; nothing is deleted on a failure.
@MainActor
final class DeleteAccountTests: XCTestCase {
    var account: FakeAccount!
    var deleted = 0
    let password = "correct horse battery staple 42"

    override func setUp() async throws {
        account = FakeAccount()
        await account.setSignedIn(email: "a@example.com")
        deleted = 0
    }

    func model() -> DeleteAccountModel {
        DeleteAccountModel(account: account, plan: PlanModel(account: account, checkout: .web)) { [unowned self] in
            self.deleted += 1
        }
    }

    func subscription(_ provider: BillingProvider, also: [BillingProvider] = []) -> BillingStatus {
        BillingStatus(
            plan: .premium,
            subscription: SubscriptionEntry(
                provider: provider, plan: .premium, status: "active", until: Date(timeIntervalSince1970: 1_800_000_000),
                canceling: false, priceLockedUntil: nil, also: also
            ),
            sells: [.appStore, .stripe],
            token: nil
        )
    }

    func testWithoutAPlanTheMasterPasswordComesNext() async {
        let model = model()
        await model.load()
        XCTAssertEqual(model.step, .intro)
        model.next()
        XCTAssertEqual(model.step, .password)
        XCTAssertFalse(model.canDelete)

        model.password = "not the password"
        await model.delete()
        XCTAssertEqual(model.error, "Wrong master password.")
        XCTAssertEqual(model.password, "", "never kept")
        XCTAssertEqual(deleted, 0)
        var signedIn = await account.signedIn
        XCTAssertTrue(signedIn)

        model.password = password
        await model.delete()
        XCTAssertNil(model.error)
        XCTAssertEqual(deleted, 1)
        signedIn = await account.signedIn
        XCTAssertFalse(signedIn)
    }

    func testAnAppStorePlanIsShownBeforeThePassword() async {
        await account.setBilling(subscription(.appStore))
        let model = model()
        await model.load()
        model.next()
        XCTAssertEqual(model.step, .plan)
        XCTAssertTrue(model.appleBills)
        XCTAssertFalse(model.endsWithRefund)
        XCTAssertNotNil(model.plan.manageURL(for: .appStore), "the Mac opens Apple's page")
        model.next()
        XCTAssertEqual(model.step, .password)
    }

    func testAStripePlanEndsWithTheUnusedDaysBack() async {
        await account.setBilling(subscription(.stripe, also: [.appStore]))
        let model = model()
        await model.load()
        model.next()
        XCTAssertEqual(model.step, .plan)
        XCTAssertTrue(model.endsWithRefund)
        XCTAssertTrue(model.appleBills, "the second subscription is Apple's to cancel")
    }

    func testNothingIsDeletedWhileAPlanCantBeEnded() async {
        let model = model()
        await model.load()
        model.next()
        await account.fail(with: "delete_unavailable")
        model.password = password
        await model.delete()
        XCTAssertEqual(model.error, PWMError(code: "delete_unavailable").message)
        XCTAssertEqual(deleted, 0)
        let signedIn = await account.signedIn
        XCTAssertTrue(signedIn)
    }
}
