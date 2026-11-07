import Foundation
@preconcurrency import PWMCore

/// One of the account's devices (D43 §6).
public struct DeviceEntry: Sendable, Equatable, Identifiable {
    public let id: String
    /// `ios`, `android`, `macos`, `windows`, `linux` or `extension`.
    public let platform: String
    /// `nil` without a name, or when it does not open on this account.
    public let name: String?
    public let added: Date
    public let lastSeen: Date
    public let revoked: Date?
    /// This device.
    public let isCurrent: Bool

    public init(id: String, platform: String, name: String?, added: Date, lastSeen: Date, revoked: Date?, isCurrent: Bool) {
        self.id = id
        self.platform = platform
        self.name = name
        self.added = added
        self.lastSeen = lastSeen
        self.revoked = revoked
        self.isCurrent = isCurrent
    }

    init(_ device: Device) {
        func date(_ ms: Int64) -> Date { Date(timeIntervalSince1970: TimeInterval(ms) / 1000) }
        self.init(
            id: device.id, platform: device.platform, name: device.name,
            added: date(device.createdAt), lastSeen: date(device.lastSeenAt),
            revoked: device.revokedAt.map(date), isCurrent: device.current
        )
    }
}

/// A request from a new device to sign in (D46).
public struct ApprovalEntry: Sendable, Equatable, Identifiable {
    public let id: String
    public let platform: String
    public let created: Date
    /// `pending`, `accepted` or `revealed`.
    public let state: String
    /// The code to compare, once this device accepted and the new device
    /// revealed its key.
    public let code: String?

    public init(id: String, platform: String, created: Date, state: String, code: String?) {
        self.id = id
        self.platform = platform
        self.created = created
        self.state = state
        self.code = code
    }
}

/// The local health checks (D22): weak passwords by item id with their
/// score (below 3), and groups of item ids sharing a password.
public struct PasswordHealth: Sendable, Equatable {
    public let weak: [String: Int]
    public let reused: [[String]]

    public init(weak: [String: Int], reused: [[String]]) {
        self.weak = weak
        self.reused = reused
    }
}

/// Export files (D24).
public enum ExportFile {
    public enum Format: Sendable {
        /// Encrypted with an export password, which opens it alone.
        case pwmx(password: Redacted<String>)
        /// Plaintext: anyone with the file can read every password.
        case json, csv

        public var fileExtension: String {
            switch self {
            case .pwmx: "pwmx"
            case .json: "json"
            case .csv: "csv"
            }
        }
    }
}

// MARK: Plan and billing (D6)

/// The account's plan. A newer server may name one this app doesn't know.
public enum Plan: Sendable, Equatable {
    case free, premium, family, teams
    case other(String)

    public init(_ name: String) {
        switch name {
        case "free": self = .free
        case "premium": self = .premium
        case "family": self = .family
        case "teams": self = .teams
        default: self = .other(name)
        }
    }

    /// The name the server uses.
    public var name: String {
        switch self {
        case .free: "free"
        case .premium: "premium"
        case .family: "family"
        case .teams: "teams"
        case let .other(name): name
        }
    }
}

/// Where a plan is bought and managed (D6 §2).
public enum BillingProvider: Sendable, Equatable {
    /// Stripe's Checkout, on the web: cancel and resume in any app.
    case stripe
    case appStore
    case play
    case other(String)

    public init(_ name: String) {
        switch name {
        case "stripe": self = .stripe
        case "app_store": self = .appStore
        case "play": self = .play
        default: self = .other(name)
        }
    }
}

/// The subscription that gives the plan (D6 §7).
public struct SubscriptionEntry: Sendable, Equatable {
    public let provider: BillingProvider
    public let plan: Plan
    /// `active`, `canceled`, `past_due`, …, as the server has it.
    public let status: String
    /// The end of the paid period.
    public let until: Date
    /// It ends at `until` instead of renewing.
    public let canceling: Bool
    public let priceLockedUntil: Date?
    /// Other subscriptions that count too, bought by mistake: where to
    /// cancel them.
    public let also: [BillingProvider]

    public init(
        provider: BillingProvider, plan: Plan, status: String, until: Date, canceling: Bool,
        priceLockedUntil: Date?, also: [BillingProvider]
    ) {
        self.provider = provider
        self.plan = plan
        self.status = status
        self.until = until
        self.canceling = canceling
        self.priceLockedUntil = priceLockedUntil
        self.also = also
    }
}

/// The account's plan, as the server has it (D6 §7).
public struct BillingStatus: Sendable, Equatable {
    public let plan: Plan
    public let subscription: SubscriptionEntry?
    /// The providers that sell plans on this server. Empty on a server
    /// without billing, where every account is Premium.
    public let sells: [BillingProvider]
    /// The account's billing token, Apple's `appAccountToken` for a
    /// purchase (D6 §3); only from `billing()`. Never logged.
    public let token: Redacted<String>?

    public init(plan: Plan, subscription: SubscriptionEntry?, sells: [BillingProvider], token: Redacted<String>?) {
        self.plan = plan
        self.subscription = subscription
        self.sells = sells
        self.token = token
    }

    init(_ info: BillingInfo) {
        func date(_ ms: Int64) -> Date { Date(timeIntervalSince1970: TimeInterval(ms) / 1000) }
        self.init(
            plan: Plan(info.plan),
            subscription: info.subscription.map {
                SubscriptionEntry(
                    provider: BillingProvider($0.provider), plan: Plan($0.plan), status: $0.status,
                    until: date($0.untilMs), canceling: $0.canceling,
                    priceLockedUntil: $0.priceLockedUntilMs.map(date), also: $0.also.map(BillingProvider.init)
                )
            },
            sells: info.sells.map(BillingProvider.init),
            token: info.token.map { Redacted($0) }
        )
    }
}
