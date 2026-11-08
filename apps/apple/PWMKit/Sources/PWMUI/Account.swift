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
    /// Deletes the account everywhere and wipes this device (D74).
    func deleteAccount(password: Redacted<String>) async throws
    func emergencyKit(paper: EmergencyKit.Paper) async throws -> Redacted<Data>

    // The vault (D43).
    func sync() async throws -> SyncResult
    /// Locked or not, with the tokens only (D50 §1).
    func backgroundSync() async throws -> SyncResult
    /// This device's APNs token on the server; nil removes it (D50 §3).
    func setPushToken(_ token: Data?, sandbox: Bool) async throws
    /// After a refused refresh token, with the master password (D50 §5).
    func signInAgain(password: Redacted<String>) async throws -> SyncResult
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

    // Import (D51).
    func guessImport(fileName: String, data: Redacted<Data>) async -> ImportGuess?
    func importColumns(_ data: Redacted<Data>) async throws -> CSVColumns
    func prepareImport(
        format: String, data: Redacted<Data>, password: Redacted<String>?, keyFile: Redacted<Data>?, columns: ColumnMap?
    ) async throws -> ImportSummary
    func finishImport() async throws -> ImportSummary
    func cancelImport() async

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

    // Suggestions above the keyboard (D47 §5).
    func suggestionsOn() async -> Bool
    func setSuggestions(on: Bool) async

    // Plan and billing (D6).
    func billing() async throws -> BillingStatus
    /// Stripe's Checkout page, for the browser.
    func checkoutURL(for plan: Plan) async throws -> URL
    func cancelSubscription() async throws -> BillingStatus
    func resumeSubscription() async throws -> BillingStatus
    /// Pays back the unused days of a Stripe or Google Play subscription and
    /// ends the plan now (D72 §1).
    func refund(provider: BillingProvider) async throws -> BillingStatus
    /// Right after StoreKit finished a purchase, before finishing it.
    func applePurchase(transactionId: String) async throws -> BillingStatus
}

extension Session: Account {}

/// A plain message for any error, never a value.
func message(for error: any Error) -> String {
    (error as? PWMError ?? PWMError(code: "internal")).message
}
