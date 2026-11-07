import Foundation
import PWMKit
import PWMUI
import StoreKit
import UIKit

/// StoreKit 2 for the iOS app (D6 §2). It lives in the app target, not the
/// package: the package also builds for the AutoFill extension and Linux.
/// A purchase carries the account's billing token as `appAccountToken`;
/// only verified transactions go on, and the server reads each one from
/// Apple before the app finishes it (D6 §3). Never logs a token or a
/// transaction.
struct AppStorePurchases: StorePurchases {
    func priceText(of productId: String) async throws -> String? {
        guard let product = try await findProduct(productId) else { return nil }
        guard let period = product.subscription?.subscriptionPeriod, period.value == 1 else {
            return product.displayPrice
        }
        switch period.unit {
        case .year: return String(localized: "\(product.displayPrice) a year")
        case .month: return String(localized: "\(product.displayPrice) a month")
        case .week: return String(localized: "\(product.displayPrice) a week")
        case .day: return String(localized: "\(product.displayPrice) a day")
        @unknown default: return product.displayPrice
        }
    }

    func purchase(_ productId: String, accountToken: UUID) async throws -> StorePurchase {
        guard let product = try await findProduct(productId) else { throw PWMError(code: "store_unavailable") }
        let result: Product.PurchaseResult
        do {
            result = try await product.purchase(options: [.appAccountToken(accountToken)])
        } catch {
            throw PWMError(code: "store_failed")
        }
        switch result {
        case let .success(verification):
            guard let transaction = Self.verified(verification) else { throw PWMError(code: "purchase_unverified") }
            return .purchased(transaction)
        case .pending:
            return .pending
        case .userCancelled:
            return .cancelled
        @unknown default:
            return .cancelled
        }
    }

    func transactions() -> AsyncStream<StoreTransaction> {
        AsyncStream { continuation in
            let task = Task {
                for await result in StoreKit.Transaction.unfinished {
                    if let transaction = Self.verified(result) { continuation.yield(transaction) }
                }
                for await result in StoreKit.Transaction.updates {
                    if let transaction = Self.verified(result) { continuation.yield(transaction) }
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    func showManageSubscriptions() async throws {
        try await Self.showManage()
    }

    /// Apple's refund sheet for the latest verified transaction of
    /// `productId` on this Apple Account (D72 §1).
    func requestRefund(productId: String) async throws -> Bool {
        guard let latest = await StoreKit.Transaction.latest(for: productId) else {
            throw PWMError(code: "refund_no_purchase")
        }
        guard case let .verified(transaction) = latest else { throw PWMError(code: "purchase_unverified") }
        return try await Self.beginRefund(transaction.id)
    }

    @MainActor
    private static func showManage() async throws {
        let scene = try activeScene()
        do {
            try await AppStore.showManageSubscriptions(in: scene)
        } catch {
            throw PWMError(code: "store_failed")
        }
    }

    @MainActor
    private static func beginRefund(_ transactionId: UInt64) async throws -> Bool {
        let scene = try activeScene()
        let status: StoreKit.Transaction.RefundRequestStatus
        do {
            status = try await StoreKit.Transaction.beginRefundRequest(for: transactionId, in: scene)
        } catch StoreKit.Transaction.RefundRequestError.duplicateRequest {
            throw PWMError(code: "refund_requested_already")
        } catch {
            throw PWMError(code: "store_failed")
        }
        switch status {
        case .success: return true
        case .userCancelled: return false
        @unknown default: return false
        }
    }

    @MainActor
    private static func activeScene() throws -> UIWindowScene {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        guard let scene = scenes.first(where: { $0.activationState == .foregroundActive }) ?? scenes.first else {
            throw PWMError(code: "store_failed")
        }
        return scene
    }

    private func findProduct(_ id: String) async throws -> Product? {
        do {
            return try await Product.products(for: [id]).first
        } catch {
            throw PWMError(code: "store_failed")
        }
    }

    private static func verified(_ result: VerificationResult<StoreKit.Transaction>) -> StoreTransaction? {
        guard case let .verified(transaction) = result else { return nil }
        return StoreTransaction(id: String(transaction.id)) { await transaction.finish() }
    }
}
