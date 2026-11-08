import Foundation
import Observation
import PWMKit

// MARK: The App Store, behind a protocol

/// A verified App Store transaction, not finished yet. Finish it once the
/// server has it (D6 §3).
public struct StoreTransaction: Sendable {
    public let id: String
    private let onFinish: @Sendable () async -> Void

    public init(id: String, finish: @escaping @Sendable () async -> Void) {
        self.id = id
        onFinish = finish
    }

    public func finish() async {
        await onFinish()
    }
}

/// How a purchase ended in the App Store's sheet.
public enum StorePurchase: Sendable {
    case purchased(StoreTransaction)
    /// Waiting for approval (Ask to Buy, or the bank); it arrives later
    /// through `transactions()`.
    case pending
    case cancelled
}

/// StoreKit 2 for the iOS app (D6 §2). The app target has the real one;
/// tests use a fake. Errors are `PWMError`s.
public protocol StorePurchases: Sendable {
    /// The product's price and period as the App Store shows it ("€29.99 a
    /// year"), or nil when the App Store doesn't sell it.
    func priceText(of productId: String) async throws -> String?
    /// Buys `productId` with the account's billing token as Apple's
    /// `appAccountToken`, so the server links it to this account (D6 §3).
    func purchase(_ productId: String, accountToken: UUID) async throws -> StorePurchase
    /// Verified transactions not finished yet, then each new one: an
    /// approved Ask to Buy, a renewal, a purchase on another device. Runs
    /// until cancelled.
    func transactions() -> AsyncStream<StoreTransaction>
    /// The App Store's own page for the user's subscriptions, where a store
    /// subscription is cancelled (D6 §5).
    func showManageSubscriptions() async throws
    /// Apple's refund sheet for the latest purchase of `productId` (D72 §1).
    /// Only Apple refunds an App Store purchase; a granted refund reaches
    /// the server through Apple's notification. False when the user closed
    /// the sheet without asking.
    func requestRefund(productId: String) async throws -> Bool
}

/// Where this app sells plans (D6 §2): the App Store on iOS, Stripe's
/// Checkout in the browser on the Mac (Developer ID, outside the Mac App
/// Store, D48 §3).
public enum Checkout: Sendable {
    case appStore(any StorePurchases)
    case web
}

// MARK: The plan in Settings

/// The account's plan, where it's managed, and the way to buy where this
/// app sells it (D6). On iOS: the App Store only, with no web prices and no
/// links to buy elsewhere. On the Mac: Stripe's Checkout in the browser, and
/// one-click cancel and resume for a Stripe subscription.
@MainActor
@Observable
public final class PlanModel {
    /// The App Store product for Premium; a placeholder until D1 and D8
    /// name the product and its price.
    public static let premiumProductId = "invalid.pwm.premium.yearly"

    /// Apple's and Google's own pages for subscriptions, opened from the Mac.
    static let appStoreSubscriptions = URL(string: "https://apps.apple.com/account/subscriptions")!
    static let playSubscriptions = URL(string: "https://play.google.com/store/account/subscriptions")!
    /// Where an App Store refund is asked for, away from StoreKit (D72 §1).
    static let appleReportAProblem = URL(string: "https://reportaproblem.apple.com")!

    public private(set) var status: BillingStatus?
    public private(set) var busy = false
    public private(set) var error: String?
    /// The App Store's price for Premium, once known.
    public private(set) var storePrice: String?
    /// The App Store doesn't sell Premium (yet), or couldn't say.
    public private(set) var storeUnavailable = false
    /// A purchase waits for approval; the plan updates when it arrives.
    public private(set) var purchasePending = false
    /// Checkout was opened in the browser: read the plan again when the
    /// app comes back.
    public private(set) var checkoutOpened = false
    /// A refund went through here: the plan ended and the money is on its way.
    public private(set) var refunded = false
    /// Apple has a refund request from its sheet; the plan changes only if
    /// Apple grants it.
    public private(set) var refundRequested = false

    private let account: any Account
    let checkout: Checkout
    /// Kept from `billing()`; the other calls don't return it.
    private var token: Redacted<String>?

    init(account: any Account, checkout: Checkout) {
        self.account = account
        self.checkout = checkout
    }

    private var store: (any StorePurchases)? {
        guard case let .appStore(store) = checkout else { return nil }
        return store
    }

    // MARK: What the screen shows

    public var plan: Plan? { status?.plan }
    public var subscription: SubscriptionEntry? { status?.subscription }

    /// A server without billing: every account has Premium there.
    public var everyAccountPremium: Bool { status?.sells.isEmpty == true }

    /// Free, no subscription, and this app sells Premium on this server.
    /// It stays true while buying, so the button doesn't move.
    public var canBuy: Bool {
        guard let status, status.plan == .free, status.subscription == nil else { return false }
        switch checkout {
        case .appStore:
            return status.sells.contains(.appStore) && appAccountToken != nil && storePrice != nil
                && !storeUnavailable && !purchasePending
        case .web:
            return status.sells.contains(.stripe)
        }
    }

    /// One click on any platform (PRICING.md §4); access lasts to `until`.
    public var canCancel: Bool {
        subscription.map { $0.provider == .stripe && !$0.canceling } ?? false
    }

    /// One click on the Mac. The iOS app doesn't offer to keep paying
    /// outside the App Store, so there it says where to resume.
    public var canResume: Bool {
        guard case .web = checkout, let subscription else { return false }
        return subscription.provider == .stripe && subscription.canceling
    }

    /// A canceling Stripe plan on iOS: resumed from another app.
    public var resumesElsewhere: Bool {
        guard case .appStore = checkout, let subscription else { return false }
        return subscription.provider == .stripe && subscription.canceling
    }

    /// iOS, for an App Store subscription: the App Store's page.
    public var canManageInStore: Bool {
        guard store != nil, let subscription else { return false }
        return subscription.provider == .appStore || subscription.also.contains(.appStore)
    }

    /// A second Stripe subscription, bought by mistake, cancels here too.
    public var canCancelSecond: Bool {
        guard let subscription, subscription.provider != .stripe else { return false }
        return subscription.also.contains(.stripe)
    }

    /// A Stripe or Google Play subscription within 30 days of its last
    /// charge, as the server says (D72 §1): one tap, on any platform.
    public var canRefund: Bool {
        guard let subscription, subscription.refundableUntil != nil else { return false }
        return subscription.provider == .stripe || subscription.provider == .play
    }

    /// iOS, for an App Store subscription: Apple's refund sheet. Shown
    /// whatever the server's window: Apple decides.
    public var canRequestStoreRefund: Bool {
        store != nil && subscription?.provider == .appStore
    }

    /// The Mac, for an App Store subscription: Apple's Report a Problem page.
    public var storeRefundURL: URL? {
        guard case .web = checkout, subscription?.provider == .appStore else { return nil }
        return Self.appleReportAProblem
    }

    /// Apple's or Google's page for a store subscription, opened in the
    /// browser from the Mac. None on iOS: the App Store's page opens over
    /// the app, and Google Play gets instructions.
    public func manageURL(for provider: BillingProvider) -> URL? {
        guard case .web = checkout else { return nil }
        switch provider {
        case .appStore: return Self.appStoreSubscriptions
        case .play: return Self.playSubscriptions
        case .stripe, .other: return nil
        }
    }

    private var appAccountToken: UUID? {
        token.flatMap { UUID(uuidString: $0.value) }
    }

    // MARK: Actions

    public func load() async {
        do {
            apply(try await account.billing())
            error = nil
        } catch {
            self.error = Self.message(for: error)
            return
        }
        checkoutOpened = false
        await loadStorePrice()
    }

    /// The App Store's price, only when there is something to buy there.
    private func loadStorePrice() async {
        guard let store, let status, status.plan == .free, status.sells.contains(.appStore), storePrice == nil else {
            return
        }
        do {
            storePrice = try await store.priceText(of: Self.premiumProductId)
        } catch {
            storePrice = nil
        }
        storeUnavailable = storePrice == nil
    }

    /// iOS: the App Store's sheet, then the transaction to the server. The
    /// Mac: returns Stripe's Checkout page for the browser.
    public func buy() async -> URL? {
        guard canBuy, !busy, let status else { return nil }
        busy = true
        defer { busy = false }
        error = nil
        switch checkout {
        case .web:
            guard status.sells.contains(.stripe) else { return nil }
            do {
                let url = try await account.checkoutURL(for: .premium)
                checkoutOpened = true
                return url
            } catch {
                self.error = Self.message(for: error)
                return nil
            }
        case let .appStore(store):
            guard let token = appAccountToken else { return nil }
            do {
                switch try await store.purchase(Self.premiumProductId, accountToken: token) {
                case let .purchased(transaction):
                    apply(try await Self.deliver(transaction, to: account))
                case .pending:
                    purchasePending = true
                case .cancelled:
                    break
                }
            } catch {
                self.error = Self.message(for: error)
            }
            return nil
        }
    }

    public func cancel() async {
        await change { try await self.account.cancelSubscription() }
    }

    public func resume() async {
        await change { try await self.account.resumeSubscription() }
    }

    /// Pays back the unused days and ends the plan now; the account keeps
    /// all its data on Free (D72 §1).
    public func refund() async {
        guard canRefund, !busy, let provider = subscription?.provider else { return }
        busy = true
        defer { busy = false }
        error = nil
        do {
            apply(try await account.refund(provider: provider))
            refunded = true
        } catch {
            self.error = Self.message(for: error)
        }
    }

    /// iOS: Apple's refund sheet, then the plan again, in case Apple already
    /// answered.
    public func requestStoreRefund() async {
        guard canRequestStoreRefund, let store else { return }
        error = nil
        do {
            if try await store.requestRefund(productId: Self.premiumProductId) { refundRequested = true }
        } catch {
            self.error = Self.message(for: error)
            return
        }
        await load()
    }

    /// iOS: the App Store's subscriptions page.
    public func manageInStore() async {
        guard let store else { return }
        error = nil
        do {
            try await store.showManageSubscriptions()
        } catch {
            self.error = Self.message(for: error)
            return
        }
        // A change there reaches the server through Apple; read it again.
        await load()
    }

    /// The app is in front again: after Checkout, the plan may have changed.
    public func returnedToApp() async {
        if checkoutOpened { await load() }
    }

    private func change(_ action: @escaping () async throws -> BillingStatus) async {
        busy = true
        defer { busy = false }
        error = nil
        do {
            apply(try await action())
        } catch {
            self.error = Self.message(for: error)
        }
    }

    /// A newer answer from the server, from here or from a transaction that
    /// finished later.
    func apply(_ status: BillingStatus) {
        if let token = status.token { self.token = token }
        self.status = status
        if status.plan != .free {
            purchasePending = false
            refunded = false
        }
        if status.subscription?.provider != .appStore { refundRequested = false }
    }

    // MARK: Transactions

    /// Codes after which the server has had its say on a transaction, so it
    /// is finished: another account's purchase is linked to that account
    /// from Apple's notifications.
    static let finishedAnyway: Set<String> = ["purchase_not_yours", "already_subscribed"]

    /// Hands an App Store transaction to the server, then finishes it (D6
    /// §3). One the server couldn't take yet (offline, still pending) stays
    /// unfinished, and StoreKit offers it again at the next unlock.
    static func deliver(_ transaction: StoreTransaction, to account: any Account) async throws -> BillingStatus {
        do {
            let status = try await account.applePurchase(transactionId: transaction.id)
            await transaction.finish()
            return status
        } catch let failure as PWMError where finishedAnyway.contains(failure.code) {
            await transaction.finish()
            throw failure
        }
    }

    // MARK: Messages

    /// The App Store's own codes. The server's billing codes (D6 §7) are in
    /// `design/messages.json`, shared with Android.
    static let messages: [String: String] = [
        "store_unavailable": "Premium isn't available in the App Store right now.",
        "store_failed": "The App Store couldn't finish. Try again.",
        "purchase_unverified": "The App Store couldn't confirm this purchase.",
        "refund_no_purchase": "This Apple Account has no purchase to refund. Use Apple's Report a Problem page.",
        "refund_requested_already": "You've already asked Apple for a refund for this purchase.",
    ]

    static func message(for error: any Error) -> String {
        if let failure = error as? PWMError, let text = messages[failure.code] { return text }
        return PWMUI.message(for: error)
    }
}
