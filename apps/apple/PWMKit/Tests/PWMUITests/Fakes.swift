import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// An account with no server: records what the screens ask for and keeps
/// items in memory, as the core's cache would.
actor FakeAccount: Account {
    var signedIn = false
    var storedEmail: String?
    var calls: [String] = []
    var unlockPassword = "correct horse battery staple 42"
    var failure: PWMError?
    var vaults = ["vault-1"]
    var stored: [String: [String: Item]] = ["vault-1": [:]]
    var syncResult: SyncResult = .synced(pulled: 0, pushed: 0, conflicts: 0, failed: 0)
    var code: (code: Redacted<String>, secondsLeft: UInt64)?
    var codeFails = false
    var nextId = 0
    let nowMs: Int64 = 1_767_225_600_000

    func setSignedIn(email: String) {
        signedIn = true
        storedEmail = email
    }

    func fail(with code: String?) {
        failure = code.map(PWMError.init(code:))
    }

    func setSyncResult(_ result: SyncResult) {
        syncResult = result
    }

    func setCode(_ value: String?, secondsLeft: UInt64 = 20, fails: Bool = false) {
        code = value.map { (code: Redacted($0), secondsLeft: secondsLeft) }
        codeFails = fails
    }

    func put(_ item: Item, in vault: String = "vault-1") {
        stored[vault, default: [:]][item.id!] = item
    }

    func storedItem(_ id: String, in vault: String = "vault-1") -> Item? {
        stored[vault]?[id]
    }

    private func check() throws {
        if let failure { throw failure }
    }

    func isSignedIn() -> Bool { signedIn }
    func email() -> String? { storedEmail }

    func signUp(email: String, password: Redacted<String>, deviceName: String) throws -> Redacted<String> {
        try check()
        calls.append("signUp \(email) \(deviceName)")
        setSignedIn(email: email)
        return Redacted("A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA")
    }

    func logIn(email: String, password: Redacted<String>, secretKey: Redacted<String>) throws -> SyncResult {
        try check()
        calls.append("logIn \(email) \(password.value) \(secretKey.value)")
        setSignedIn(email: email)
        return .synced(pulled: 0, pushed: 0, conflicts: 0, failed: 0)
    }

    func unlock(password: Redacted<String>) throws {
        guard password.value == unlockPassword else { throw PWMError(code: "wrong_password") }
        calls.append("unlock")
    }

    func lock() { calls.append("lock") }

    func signOut() {
        calls.append("signOut")
        signedIn = false
        storedEmail = nil
        stored = [:]
    }

    func emergencyKit(paper: EmergencyKit.Paper) throws -> Redacted<Data> {
        try check()
        calls.append("kit \(paper)")
        return Redacted(Data("%PDF-fake".utf8))
    }

    func sync() throws -> SyncResult {
        try check()
        calls.append("sync")
        return syncResult
    }

    func vaultIds() -> [String] { vaults }

    func items(in vaultId: String) throws -> [Item] {
        try check()
        return Array((stored[vaultId] ?? [:]).values)
    }

    func newItem(_ kind: Item.Kind, title: String) -> Item {
        nextId += 1
        return Item(kind: kind, title: title, id: "new-\(nextId)", nowMs: nowMs)
    }

    func save(_ item: Item, in vaultId: String) throws -> String {
        try check()
        guard item.isEditable else { throw PWMError(code: "read_only_item") }
        calls.append("save \(item.type)")
        put(item, in: vaultId)
        return item.id!
    }

    func setTrashed(_ itemId: String, in vaultId: String, trashed: Bool) throws {
        try check()
        guard let item = stored[vaultId]?[itemId] else { throw PWMError(code: "unknown_item") }
        var json = item.json
        json["trashed_at"] = trashed ? .int(nowMs) : .null
        put(try Item(json: try JSONValue.object(json).text()), in: vaultId)
        calls.append(trashed ? "trash" : "restore")
    }

    func delete(_ itemId: String, in vaultId: String) throws {
        try check()
        stored[vaultId]?[itemId] = nil
        calls.append("delete")
    }

    func purgeTrash() -> Int {
        calls.append("purge")
        return 0
    }

    func currentCode(_ itemId: String, in vaultId: String) throws -> (code: Redacted<String>, secondsLeft: UInt64)? {
        if codeFails { throw PWMError(code: "invalid_item") }
        return code
    }
}

@MainActor
final class FakePasteboard: SecretPasteboard {
    var copied: [(String, TimeInterval)] = []

    func copy(_ value: Redacted<String>, clearAfter seconds: TimeInterval) {
        copied.append((value.value, seconds))
    }
}

let strong = "correct horse battery staple 42"
let kitKey = "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA"

/// A login item for tests.
func loginItem(_ id: String, _ title: String, username: String = "", url: String? = nil, password: String = "") -> Item {
    var item = Item(kind: .login, title: title, id: id, nowMs: 1)
    var login = Login()
    login.username = username
    login.password = password
    if let url { login.urls = [LoginURL(url: url)] }
    item.login = login
    return item
}
