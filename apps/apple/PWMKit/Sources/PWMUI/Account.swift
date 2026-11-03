import Foundation
import PWMKit

/// What the screens need from a signed-in device. `Session` is the real one;
/// tests use a fake, so the view models run without a server or SwiftUI.
public protocol Account: AnyObject, Sendable {
    func isSignedIn() async throws -> Bool
    func email() async throws -> String?
    func signUp(email: String, password: Redacted<String>, deviceName: String) async throws -> Redacted<String>
    func logIn(email: String, password: Redacted<String>, secretKey: Redacted<String>, deviceName: String) async throws -> SyncResult
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

    // Devices, health and export (D43 §6-8).
    func devices() async throws -> [DeviceEntry]
    func renameDevice(_ deviceId: String, to name: String) async throws
    func revokeDevice(_ deviceId: String) async throws
    func passwordHealth() async throws -> PasswordHealth
    func breachedPasswords() async throws -> [String: UInt64]
    func authorizeExport(password: Redacted<String>) async throws
    func export(_ format: ExportFile.Format) async throws -> Redacted<Data>

    // Approving a new device (D46).
    func askForApproval(email: String, onCode: @escaping @Sendable (String) async -> Void) async throws -> Redacted<String>
    func approvalRequests() async throws -> [ApprovalEntry]
    func acceptApproval(_ id: String) async throws
    func completeApproval(_ id: String) async throws
    func denyApproval(_ id: String) async throws

    // Face ID / Touch ID (D45).
    func biometricState() async throws -> BiometricState
    func enableBiometricUnlock(reason: String) async throws
    func disableBiometricUnlock() async throws
    func unlockWithBiometrics(reason: String) async throws
}

extension Session: Account {}

/// A plain message for any error, never a value.
func message(for error: any Error) -> String {
    (error as? PWMError ?? PWMError(code: "internal")).message
}
