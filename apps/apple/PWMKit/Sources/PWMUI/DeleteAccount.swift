import Foundation
import Observation
import PWMKit

/// Deleting the account (D74 §7), from Settings: what will happen, with
/// export offered first; the plan, when there is one; then the master
/// password. No typed word and no countdown: the password is the
/// confirmation.
@MainActor
@Observable
public final class DeleteAccountModel {
    public enum Step: Sendable, Equatable {
        case intro, plan, password
    }

    public private(set) var step: Step = .intro
    public var password = ""
    public private(set) var busy = false
    public private(set) var error: String?

    /// The plan section's model: who bills the plan, and where to cancel it.
    public let plan: PlanModel
    private let account: any Account
    private let deleted: @MainActor () -> Void

    init(account: any Account, plan: PlanModel, deleted: @escaping @MainActor () -> Void) {
        self.account = account
        self.plan = plan
        self.deleted = deleted
    }

    /// Reads the plan; without it, the plan step is skipped and the server
    /// still ends a Stripe or Google Play plan first.
    public func load() async {
        await plan.load()
    }

    /// Who bills the plan, and the other subscriptions that count too.
    public var billedBy: [BillingProvider] {
        guard let subscription = plan.subscription else { return [] }
        return [subscription.provider] + subscription.also
    }

    /// Stripe or Google Play: the plan ends with the unused days back.
    public var endsWithRefund: Bool {
        billedBy.contains { $0 == .stripe || $0 == .play }
    }

    /// Only Apple can end an App Store plan.
    public var appleBills: Bool { billedBy.contains(.appStore) }

    public func next() {
        switch step {
        case .intro: step = billedBy.isEmpty ? .password : .plan
        case .plan, .password: step = .password
        }
    }

    public var canDelete: Bool { !busy && !password.isEmpty }

    /// Proves the password to the server and deletes everything; this
    /// device is wiped and the app goes back to its first screen.
    public func delete() async {
        guard canDelete else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            try await account.deleteAccount(password: Redacted(password))
            password = ""
            deleted()
        } catch {
            password = ""
            self.error = message(for: error)
        }
    }
}
