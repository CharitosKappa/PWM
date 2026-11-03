import Foundation
@preconcurrency import PWMCore

/// A login the AutoFill extension may fill (D47 §4): never its password,
/// which is read only when the user taps it.
public struct FillChoice: Sendable, Equatable, Identifiable {
    public let vaultId: String
    public let itemId: String
    public let title: String
    public let username: String

    public var id: String { Suggestion.record(vaultId: vaultId, itemId: itemId) }

    public init(vaultId: String, itemId: String, title: String, username: String) {
        self.vaultId = vaultId
        self.itemId = itemId
        self.title = title
        self.username = username
    }
}

/// What a tap fills: the username and password, and whether the login has
/// a one-time code to copy.
public struct FillCredential: Sendable {
    public let username: String
    public let password: Redacted<String>
    public let hasCode: Bool
}

/// A passkey a sign-in may use, chosen after Face ID.
public struct PasskeyChoice: Sendable, Equatable, Identifiable {
    public let vaultId: String
    public let itemId: String
    public let credentialId: Data
    public let userName: String

    public var id: Data { credentialId }

    public init(vaultId: String, itemId: String, credentialId: Data, userName: String) {
        self.vaultId = vaultId
        self.itemId = itemId
        self.credentialId = credentialId
        self.userName = userName
    }
}

/// What a passkey registration hands back to iOS.
public struct NewPasskey: Sendable, Equatable {
    public let credentialId: Data
    public let attestationObject: Data
}

/// What a passkey sign-in hands back to iOS.
public struct PasskeySignature: Sendable, Equatable {
    public let credentialId: Data
    public let userHandle: Data
    public let authenticatorData: Data
    public let signature: Data
}

extension Session {
    /// The page or app address for a service identifier iOS passes: a URL
    /// as it is, a domain as `https://domain`.
    static func pageURL(for service: String) -> String {
        service.contains("://") ? service : "https://\(service)"
    }

    /// The logins that match any of `services` (D33), best first, without
    /// the ones the matcher refuses (another site, a blocked page).
    public func fillChoices(for services: [String]) throws -> [FillChoice] {
        var seen = Set<String>()
        var out: [FillChoice] = []
        for service in services {
            let matches = try core { try client.autofillMatches(pageUrl: Session.pageURL(for: service), frameUrl: nil) }
            for m in matches where !m.decision.hasPrefix("refuse") {
                let choice = FillChoice(vaultId: m.vaultId, itemId: m.itemId, title: m.title, username: m.username)
                if seen.insert(choice.id).inserted { out.append(choice) }
            }
        }
        return out
    }

    /// Every login with a password, for the search below the matches.
    public func allFillChoices() throws -> [FillChoice] {
        var out: [FillChoice] = []
        for vaultId in try vaultIds() {
            for item in try items(in: vaultId) where item.trashedAtMs == nil {
                guard let login = item.login, !login.password.isEmpty, let id = item.id else { continue }
                out.append(FillChoice(vaultId: vaultId, itemId: id, title: item.title, username: login.username))
            }
        }
        return out.sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    /// The username and password to fill, read when the user tapped.
    public func fillCredential(_ itemId: String, in vaultId: String) throws -> FillCredential {
        guard let item = try item(itemId, in: vaultId), item.trashedAtMs == nil, let login = item.login else {
            throw PWMError(code: "unknown_item")
        }
        return FillCredential(username: login.username, password: Redacted(login.password), hasCode: login.totp != nil)
    }

    /// The passkeys for `rpId`; only `allowed` ones when the site lists them.
    public func passkeyChoices(rpId: String, allowed: [Data]) throws -> [PasskeyChoice] {
        try core { try client.credentialIdentities() }
            .filter { $0.kind == "passkey" && $0.service == rpId && (allowed.isEmpty || allowed.contains($0.credentialId)) }
            .map { PasskeyChoice(vaultId: $0.vaultId, itemId: $0.itemId, credentialId: $0.credentialId, userName: $0.user) }
    }

    /// Makes and stores a passkey in the first vault after the user
    /// confirmed (D47 §6). iOS already checked `rpId` against the app or
    /// site; the core still refuses one that is not a registrable domain.
    public func registerPasskey(rpId: String, userName: String, userHandle: Data) throws -> NewPasskey {
        guard let vaultId = try vaultIds().first else { throw PWMError(code: "unknown_vault") }
        let made = try core {
            try client.passkeyRegisterHashed(
                vaultId: vaultId, rpId: rpId, userName: userName, userHandle: userHandle, nowMs: now()
            )
        }
        refreshSuggestions()
        return NewPasskey(credentialId: made.credentialId, attestationObject: made.attestationObject)
    }

    /// Signs a sign-in iOS built, given its client data hash.
    public func signWithPasskey(_ choice: PasskeyChoice, rpId: String, clientDataHash: Data) throws -> PasskeySignature {
        let signed = try core {
            try client.passkeyAssertHashed(
                vaultId: choice.vaultId, itemId: choice.itemId, credentialId: choice.credentialId,
                rpId: rpId, clientDataHash: clientDataHash
            )
        }
        return PasskeySignature(
            credentialId: signed.credentialId, userHandle: signed.userHandle,
            authenticatorData: signed.authenticatorData, signature: signed.signature
        )
    }
}
