import Foundation
import PWMKit

/// What the screens need from a signed-in device. `Session` is the real one;
/// tests use a fake, so the view models run without a server or SwiftUI.
public protocol Account: AnyObject, Sendable {
    func isSignedIn() async throws -> Bool
    func email() async throws -> String?
    func signUp(email: String, password: Redacted<String>, deviceName: String) async throws -> Redacted<String>
    func logIn(email: String, password: Redacted<String>, secretKey: Redacted<String>) async throws -> SyncResult
    func unlock(password: Redacted<String>) async throws
    func lock() async
    func signOut() async throws
    func emergencyKit(paper: EmergencyKit.Paper) async throws -> Redacted<Data>

    // The vault (D43).
    func sync() async throws -> SyncResult
    func vaultIds() async throws -> [String]
    /// Every item of a vault, trashed ones included.
    func items(in vaultId: String) async throws -> [Item]
    func newItem(_ kind: Item.Kind, title: String) async throws -> Item
    func save(_ item: Item, in vaultId: String) async throws -> String
    func setTrashed(_ itemId: String, in vaultId: String, trashed: Bool) async throws
    func delete(_ itemId: String, in vaultId: String) async throws
    func purgeTrash() async throws -> Int
    func currentCode(_ itemId: String, in vaultId: String) async throws -> (code: Redacted<String>, secondsLeft: UInt64)?
}

extension Session: Account {}

/// A plain message for any error, never a value.
func message(for error: any Error) -> String {
    (error as? PWMError ?? PWMError(code: "internal")).message
}
