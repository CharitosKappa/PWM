import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// The plan in Settings (D6): the App Store on iOS, Stripe's Checkout on
/// the Mac, and where a plan bought elsewhere is managed.
@MainActor
final class PlanTests: XCTestCase {
    var account: FakeAccount!
    var store: FakeStore!

    override func setUp() async throws {
        account = FakeAccount()
        store = FakeStore()
    }

    func iOS() -> PlanModel { PlanModel(account: account, checkout: .appStore(store)) }
    func mac() -> PlanModel { PlanModel(account: account, checkout: .web) }

    func status(
        _ plan: Plan = .premium, _ subscription: SubscriptionEntry? = nil, sells: [BillingProvider] = [.appStore, .stripe]
    ) -> BillingStatus {
        BillingStatus(plan: plan, subscription: subscription, sells: sells, token: Redacted(billingToken))
    }

    // MARK: Buying on iOS

    func testAnAppStorePurchaseCarriesTheBillingTokenAndIsFinishedAfterTheServerHasIt() async throws {
        let plan = iOS()
        await plan.load()
        XCTAssertEqual(plan.plan, .free)
        XCTAssertEqual(plan.storePrice, "€29.99 a year")
        XCTAssertTrue(plan.canBuy)

        let url = await plan.buy()
        XCTAssertNil(url, "nothing opens outside the App Store")
        let purchases = await store.purchases
        XCTAssertEqual(purchases, ["\(PlanModel.premiumProductId) \(billingToken)"])
        let calls = await account.calls
        XCTAssertEqual(calls, ["billing", "applePurchase 2000000123"])
        let finished = await store.finished
        XCTAssertEqual(finished, ["2000000123"])
        XCTAssertEqual(plan.plan, .premium)
        XCTAssertEqual(plan.subscription?.provider, .appStore)
        XCTAssertFalse(plan.canBuy)
        XCTAssertTrue(plan.canManageInStore)
        XCTAssertFalse(plan.canCancel, "only the App Store cancels its subscriptions")
        XCTAssertNil(plan.error)
    }

    func testATransactionTheServerCouldNotTakeStaysUnfinished() async throws {
        let plan = iOS()
        await plan.load()
        await account.failBilling(with: "billing_unavailable")
        _ = await plan.buy()
        let finished = await store.finished
        XCTAssertEqual(finished, [], "StoreKit offers it again at the next unlock")
        XCTAssertEqual(plan.plan, .free)
        XCTAssertEqual(plan.error, "Plans can't be changed right now. Try again later.")
    }

    func testAnotherAccountsPurchaseIsFinishedAndSaysSo() async throws {
        let plan = iOS()
        await plan.load()
        await account.failBilling(with: "purchase_not_yours")
        _ = await plan.buy()
        let finished = await store.finished
        XCTAssertEqual(finished, ["2000000123"])
        XCTAssertEqual(plan.error, "This purchase belongs to another account.")
    }

    func testAPendingPurchaseWaitsAndACancelledOneChangesNothing() async throws {
        let plan = iOS()
        await plan.load()
        await store.set(outcome: .cancelled)
        _ = await plan.buy()
        XCTAssertFalse(plan.purchasePending)
        XCTAssertNil(plan.error)

        await store.set(outcome: .pending)
        _ = await plan.buy()
        XCTAssertTrue(plan.purchasePending)
        XCTAssertFalse(plan.canBuy, "no second purchase while one waits")
        let calls = await account.calls
        XCTAssertEqual(calls, ["billing"], "nothing goes to the server until it finishes")
    }

    func testAStoreFailureShowsAPlainMessage() async throws {
        let plan = iOS()
        await plan.load()
        await store.set(outcome: .fails("store_failed"))
        _ = await plan.buy()
        XCTAssertEqual(plan.error, "The App Store couldn't finish. Try again.")
        XCTAssertTrue(plan.canBuy)
    }

    func testNoBuyButtonWhenTheAppStoreDoesNotSellPremium() async throws {
        await store.set(price: nil)
        let plan = iOS()
        await plan.load()
        XCTAssertTrue(plan.storeUnavailable)
        XCTAssertFalse(plan.canBuy)
        let bought = await plan.buy()
        XCTAssertNil(bought)
        let purchases = await store.purchases
        XCTAssertEqual(purchases, [])
    }

    func testTheIOSAppNeverOffersTheWebCheckout() async throws {
        await account.setBilling(status(.free, sells: [.stripe]))
        let plan = iOS()
        await plan.load()
        XCTAssertFalse(plan.canBuy)
        let bought = await plan.buy()
        XCTAssertNil(bought)
        let calls = await account.calls
        XCTAssertEqual(calls, ["billing"])
        XCTAssertNil(plan.manageURL(for: .appStore))
        XCTAssertNil(plan.manageURL(for: .play))
    }

    func testTransactionsThatFinishLaterGoToTheServerWhileUnlocked() async throws {
        let store = FakeStore(queued: ["2000000200"])
        await account.setSignedIn(email: "ada@example.com")
        let app = AppModel(
            makeAccount: { [unowned self] _ in self.account },
            defaults: UserDefaults(suiteName: "pwm-plan-\(UUID().uuidString)")!,
            pasteboard: FakePasteboard(),
            deviceName: "iPhone",
            checkout: .appStore(store)
        )
        await app.start()
        XCTAssertNil(app.storeUpdates, "nothing is sent while locked")
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        await app.storeUpdates?.value
        let calls = await account.calls
        XCTAssertTrue(calls.contains("applePurchase 2000000200"))
        let finished = await store.finished
        XCTAssertEqual(finished, ["2000000200"])
        XCTAssertEqual(app.settings?.plan.plan, .premium)

        await app.lock()
        XCTAssertNil(app.storeUpdates)
    }

    // MARK: Buying and cancelling on the Mac

    func testTheMacOpensStripesCheckoutAndReadsThePlanAgainOnReturn() async throws {
        let plan = mac()
        await plan.load()
        XCTAssertTrue(plan.canBuy)
        XCTAssertNil(plan.storePrice, "no App Store on the Mac")
        let url = await plan.buy()
        XCTAssertEqual(url, checkoutPage)
        XCTAssertTrue(plan.checkoutOpened)

        await account.setBilling(status(.premium, subscription(.stripe)))
        await plan.returnedToApp()
        XCTAssertEqual(plan.plan, .premium)
        XCTAssertFalse(plan.checkoutOpened)
        await plan.returnedToApp()
        let calls = await account.calls
        XCTAssertEqual(calls, ["billing", "checkout premium", "billing"], "read again only after Checkout")
        let purchases = await store.purchases
        XCTAssertEqual(purchases, [])
    }

    func testNoWebCheckoutWhereTheServerDoesNotSellOnTheWeb() async throws {
        await account.setBilling(status(.free, sells: [.appStore]))
        let plan = mac()
        await plan.load()
        XCTAssertFalse(plan.canBuy)
    }

    func testAStripePlanCancelsAndResumesWithOneClick() async throws {
        await account.setBilling(status(.premium, subscription(.stripe)))
        let plan = mac()
        await plan.load()
        XCTAssertTrue(plan.canCancel)
        XCTAssertFalse(plan.canResume)

        await plan.cancel()
        XCTAssertEqual(plan.subscription?.canceling, true)
        XCTAssertEqual(plan.plan, .premium, "access lasts to the end of the period")
        XCTAssertFalse(plan.canCancel)
        XCTAssertTrue(plan.canResume)

        await plan.resume()
        XCTAssertEqual(plan.subscription?.canceling, false)
        XCTAssertTrue(plan.canCancel)
        let calls = await account.calls
        XCTAssertEqual(calls, ["billing", "cancel", "resume"])
    }

    func testIOSCancelsAStripePlanButSaysWhereToResumeIt() async throws {
        await account.setBilling(status(.premium, subscription(.stripe)))
        let plan = iOS()
        await plan.load()
        XCTAssertTrue(plan.canCancel)
        await plan.cancel()
        XCTAssertFalse(plan.canResume)
        XCTAssertTrue(plan.resumesElsewhere)
        XCTAssertFalse(plan.canManageInStore)
    }

    func testAStoreSubscriptionIsManagedInItsStore() async throws {
        await account.setBilling(status(.premium, subscription(.appStore)))
        let onMac = mac()
        await onMac.load()
        XCTAssertFalse(onMac.canCancel)
        XCTAssertFalse(onMac.canManageInStore, "no StoreKit on the Mac")
        XCTAssertEqual(onMac.manageURL(for: .appStore), PlanModel.appStoreSubscriptions)

        let onIOS = iOS()
        await onIOS.load()
        XCTAssertTrue(onIOS.canManageInStore)
        await onIOS.manageInStore()
        let managed = await store.managed
        XCTAssertEqual(managed, 1)

        await account.setBilling(status(.premium, subscription(.play)))
        await onMac.load()
        XCTAssertEqual(onMac.manageURL(for: .play), PlanModel.playSubscriptions)
        await onIOS.load()
        XCTAssertFalse(onIOS.canManageInStore)
        XCTAssertFalse(onIOS.canCancel)
        XCTAssertFalse(onIOS.canBuy, "a plan bought elsewhere isn't offered again")
    }

    func testASecondSubscriptionIsPointedOutWithWhereToCancelIt() async throws {
        await account.setBilling(status(.premium, subscription(.appStore, also: [.stripe])))
        let plan = mac()
        await plan.load()
        XCTAssertEqual(plan.subscription?.also, [.stripe])
        XCTAssertTrue(plan.canCancelSecond)
        await plan.cancel()
        XCTAssertEqual(plan.subscription?.also, [])

        await account.setBilling(status(.premium, subscription(.stripe, also: [.appStore])))
        let onIOS = iOS()
        await onIOS.load()
        XCTAssertFalse(onIOS.canCancelSecond, "the Stripe one is the main one")
        XCTAssertTrue(onIOS.canManageInStore, "the App Store one is managed there")
    }

    func testAStoreSubscriptionCannotBeCancelledHere() async throws {
        await account.setBilling(status(.premium, subscription(.play)))
        let plan = mac()
        await plan.load()
        await plan.cancel()
        XCTAssertEqual(plan.error, "Change this plan where you bought it.")
    }

    // MARK: Servers

    func testAServerWithoutBillingGivesEveryAccountPremium() async throws {
        await account.setBilling(BillingStatus(plan: .premium, subscription: nil, sells: [], token: Redacted(billingToken)))
        for plan in [iOS(), mac()] {
            await plan.load()
            XCTAssertTrue(plan.everyAccountPremium)
            XCTAssertFalse(plan.canBuy)
            XCTAssertFalse(plan.canCancel)
        }
        let purchases = await store.purchases
        XCTAssertEqual(purchases, [])
    }

    func testAnUnreachableServerSaysToTryLater() async throws {
        await account.failBilling(with: "billing_unavailable")
        let plan = iOS()
        await plan.load()
        XCTAssertNil(plan.plan)
        XCTAssertEqual(plan.error, "Plans can't be changed right now. Try again later.")
        XCTAssertFalse(plan.canBuy)
    }

    func testTheTokenIsNeverPrinted() {
        let info = status()
        XCTAssertFalse(String(describing: info).contains(billingToken))
        XCTAssertFalse(String(reflecting: info).contains(billingToken))
    }
}
