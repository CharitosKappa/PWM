import Foundation
@preconcurrency import PWMCore

/// One suggestion iOS may show above the keyboard before unlock (D47 §5):
/// a domain or RP id and a username, never a password or a title.
public struct Suggestion: Sendable, Equatable {
    public enum Kind: String, Sendable {
        case password, passkey
    }

    public let kind: Kind
    /// A domain for a password; the RP id for a passkey.
    public let service: String
    public let user: String
    /// `vaultId/itemId`: what iOS hands back when one is chosen.
    public let record: String
    /// Passkeys only; empty for a password.
    public let credentialId: Data
    public let userHandle: Data

    public init(kind: Kind, service: String, user: String, record: String, credentialId: Data = Data(), userHandle: Data = Data()) {
        self.kind = kind
        self.service = service
        self.user = user
        self.record = record
        self.credentialId = credentialId
        self.userHandle = userHandle
    }

    init?(_ identity: CredentialIdentity) {
        guard let kind = Kind(rawValue: identity.kind) else { return nil }
        self.init(
            kind: kind, service: identity.service, user: identity.user,
            record: Suggestion.record(vaultId: identity.vaultId, itemId: identity.itemId),
            credentialId: identity.credentialId, userHandle: identity.userHandle
        )
    }

    public static func record(vaultId: String, itemId: String) -> String { "\(vaultId)/\(itemId)" }

    /// The vault and item of a record, if it is one of ours.
    public static func parse(record: String) -> (vaultId: String, itemId: String)? {
        let parts = record.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count == 2, !parts[0].isEmpty, !parts[1].isEmpty else { return nil }
        return (String(parts[0]), String(parts[1]))
    }
}

/// The system's list of suggestions: `ASCredentialIdentityStore` on iOS and macOS,
/// a fake in tests.
public protocol SuggestionStore: Sendable {
    /// Replaces the whole list; an empty one clears it.
    func replace(with suggestions: [Suggestion]) async
}

/// Keeps the system's list current (D47 §5): the latest list always wins,
/// sent one at a time. The switch ("Suggest logins above the keyboard",
/// on by default) lives in `defaults`.
public final class Suggestions: @unchecked Sendable {
    static let switchKey = "suggestLogins"

    private let store: any SuggestionStore
    private let defaults: UserDefaults
    private let lock = NSLock()
    private var pending: [Suggestion]?
    private var sending: Task<Void, Never>?

    public init(store: any SuggestionStore, defaults: UserDefaults = .standard) {
        self.store = store
        self.defaults = defaults
    }

    public var isOn: Bool {
        get { defaults.object(forKey: Self.switchKey) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Self.switchKey) }
    }

    /// Sends `list` (or nothing, when the switch is off) after any send
    /// already under way; a newer list replaces one still waiting.
    public func update(_ list: [Suggestion]) {
        let next = isOn ? list : []
        lock.lock()
        defer { lock.unlock() }
        pending = next
        if sending == nil {
            sending = Task { await self.send() }
        }
    }

    public func clear() {
        lock.lock()
        defer { lock.unlock() }
        pending = []
        if sending == nil {
            sending = Task { await self.send() }
        }
    }

    private func send() async {
        while let next = take() {
            await store.replace(with: next)
        }
    }

    private func take() -> [Suggestion]? {
        lock.lock()
        defer { lock.unlock() }
        guard let next = pending else {
            sending = nil
            return nil
        }
        pending = nil
        return next
    }

    /// Waits until every update so far reached the store (tests).
    public func settle() async {
        while let task = currentSend() {
            await task.value
        }
    }

    private func currentSend() -> Task<Void, Never>? {
        lock.lock()
        defer { lock.unlock() }
        return sending
    }
}

#if (os(iOS) || os(macOS)) && canImport(AuthenticationServices)
import AuthenticationServices

/// The system's store. When PWM is not the AutoFill provider it refuses
/// changes; the list is sent again after the next vault change.
public struct SystemSuggestionStore: SuggestionStore {
    public init() {}

    public func replace(with suggestions: [Suggestion]) async {
        let store = ASCredentialIdentityStore.shared
        guard await store.state().isEnabled else { return }
        let identities = suggestions.map { s -> any ASCredentialIdentity in
            switch s.kind {
            case .password:
                return ASPasswordCredentialIdentity(
                    serviceIdentifier: ASCredentialServiceIdentifier(identifier: s.service, type: .domain),
                    user: s.user, recordIdentifier: s.record
                )
            case .passkey:
                return ASPasskeyCredentialIdentity(
                    relyingPartyIdentifier: s.service, userName: s.user,
                    credentialID: s.credentialId, userHandle: s.userHandle, recordIdentifier: s.record
                )
            }
        }
        try? await store.replaceCredentialIdentities(identities)
    }
}
#endif
