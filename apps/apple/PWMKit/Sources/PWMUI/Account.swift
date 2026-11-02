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
    /// Items not in the trash, across every vault.
    func itemCount() async throws -> Int
}

extension Session: Account {
    public func itemCount() throws -> Int {
        try vaultIds().reduce(0) { count, vault in
            count + (try items(in: vault).filter { $0.trashedAtMs == nil }.count)
        }
    }
}

/// A plain message for any error, never a value.
func message(for error: any Error) -> String {
    (error as? PWMError ?? PWMError(code: "internal")).message
}
