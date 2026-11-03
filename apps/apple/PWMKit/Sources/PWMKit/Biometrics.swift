import Foundation

/// Face ID or Touch ID, as the device offers it (D45).
public enum BiometricKind: String, Sendable, Equatable {
    case faceID, touchID
}

/// Whether biometric unlock can be turned on here.
public enum BiometricAvailability: Sendable, Equatable {
    case available(BiometricKind)
    /// No enrolled face or finger, no passcode, or no Secure Enclave
    /// (the simulator): the switch says why.
    case unavailable(reason: String)
}

/// The hardware key that protects the biometric unlock key (D45 §2): a
/// Secure Enclave key on Apple devices, a fake in tests. It wraps and
/// unwraps the core's 32-byte key; the vault crypto stays in the core.
public protocol BiometricKeyStore: Sendable {
    func availability() -> BiometricAvailability
    /// Makes a new hardware key (replacing an old one) and wraps `secret`.
    func wrap(_ secret: Redacted<Data>) throws -> Data
    /// Asks for Face ID or Touch ID, then unwraps. Throws
    /// `biometric_cancelled`, or `biometric_changed` when the key no longer
    /// opens (a face or finger was added, or the key is gone).
    func unwrap(_ wrapped: Data, reason: String) async throws -> Redacted<Data>
    func delete() throws
}

/// Where biometric unlock stands for the signed-in account.
public enum BiometricState: Sendable, Equatable {
    case unavailable(reason: String)
    case off(BiometricKind)
    case on(BiometricKind)
}

public enum Biometrics {
    /// The master password is asked again after this long (D45 §4).
    public static let masterPasswordEveryMs: Int64 = 14 * 24 * 60 * 60 * 1000

    /// The Secure Enclave on Apple platforms, nothing elsewhere.
    public static var platform: any BiometricKeyStore {
        #if canImport(LocalAuthentication) && canImport(Security)
        SecureEnclaveKeyStore()
        #else
        NoBiometrics()
        #endif
    }
}

/// For platforms and builds without biometric hardware.
public struct NoBiometrics: BiometricKeyStore {
    let reason: String

    public init(reason: String = "This device can't use Face ID or Touch ID for PWM.") {
        self.reason = reason
    }

    public func availability() -> BiometricAvailability { .unavailable(reason: reason) }
    public func wrap(_ secret: Redacted<Data>) throws -> Data { throw PWMError(code: "biometric_unavailable") }
    public func unwrap(_ wrapped: Data, reason: String) async throws -> Redacted<Data> {
        throw PWMError(code: "biometric_unavailable")
    }
    public func delete() throws {}
}

#if canImport(LocalAuthentication) && canImport(Security)
import LocalAuthentication
import Security

/// A P-256 key in the Secure Enclave, usable only after Face ID or Touch ID
/// with the current enrolment, on this device only (D45 §2). The 32-byte
/// unlock key is encrypted to its public half with the system's ECIES; only
/// that ciphertext is kept, and decrypting it needs the biometric check.
public struct SecureEnclaveKeyStore: BiometricKeyStore {
    public let tag: String
    /// The Keychain group shared with the AutoFill extension (D47 §2), so
    /// it can ask for Face ID too. Finding and deleting the key look in
    /// every group this app may use.
    public let accessGroup: String?

    public init(tag: String = "pwm.biometric-unlock", accessGroup: String? = nil) {
        self.tag = tag
        self.accessGroup = accessGroup
    }

    private static var algorithm: SecKeyAlgorithm { .eciesEncryptionCofactorVariableIVX963SHA256AESGCM }

    public func availability() -> BiometricAvailability {
        #if targetEnvironment(simulator)
        return .unavailable(reason: "Face ID unlock needs a device with a Secure Enclave.")
        #else
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
            if (error as? LAError)?.code == .passcodeNotSet {
                return .unavailable(reason: "Set a device passcode to use Face ID or Touch ID.")
            }
            return .unavailable(reason: "Set up Face ID or Touch ID in Settings first.")
        }
        switch context.biometryType {
        case .faceID: return .available(.faceID)
        case .touchID: return .available(.touchID)
        default: return .unavailable(reason: "This device has no Face ID or Touch ID.")
        }
        #endif
    }

    public func wrap(_ secret: Redacted<Data>) throws -> Data {
        try delete()
        var error: Unmanaged<CFError>?
        guard let access = SecAccessControlCreateWithFlags(
            nil, kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, [.privateKeyUsage, .biometryCurrentSet], &error
        ) else { throw PWMError(code: "biometric_unavailable") }
        var keyAttributes: [CFString: Any] = [
            kSecAttrIsPermanent: true,
            kSecAttrApplicationTag: Data(tag.utf8),
            kSecAttrAccessControl: access,
        ]
        if let accessGroup {
            keyAttributes[kSecAttrAccessGroup] = accessGroup
        }
        var attributes: [CFString: Any] = [
            kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits: 256,
            kSecAttrTokenID: kSecAttrTokenIDSecureEnclave,
            kSecPrivateKeyAttrs: keyAttributes,
        ]
        // On macOS, Secure Enclave keys live in the data-protection Keychain
        // only (D48 §2); iOS has no other.
        #if os(macOS)
        attributes[kSecUseDataProtectionKeychain] = true
        #endif
        guard let privateKey = SecKeyCreateRandomKey(attributes as CFDictionary, &error),
              let publicKey = SecKeyCopyPublicKey(privateKey),
              let wrapped = SecKeyCreateEncryptedData(publicKey, Self.algorithm, secret.value as CFData, &error)
        else { throw PWMError(code: "biometric_unavailable") }
        return wrapped as Data
    }

    public func unwrap(_ wrapped: Data, reason: String) async throws -> Redacted<Data> {
        let tag = tag
        // The prompt blocks its thread until the user answers.
        return try await Task.detached {
            let context = LAContext()
            context.localizedReason = reason
            var query: [CFString: Any] = [
                kSecClass: kSecClassKey,
                kSecAttrApplicationTag: Data(tag.utf8),
                kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
                kSecReturnRef: true,
                kSecUseAuthenticationContext: context,
            ]
            #if os(macOS)
            query[kSecUseDataProtectionKeychain] = true
            #endif
            var item: CFTypeRef?
            guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess, let found = item else {
                throw PWMError(code: "biometric_changed")
            }
            // A key query with kSecReturnRef returns a SecKey.
            let privateKey = found as! SecKey
            var error: Unmanaged<CFError>?
            guard let secret = SecKeyCreateDecryptedData(privateKey, Self.algorithm, wrapped as CFData, &error) else {
                let code = (error?.takeRetainedValue() as Error?).map { ($0 as NSError).code } ?? 0
                let cancelled = [
                    Int(errSecUserCanceled), LAError.userCancel.rawValue, LAError.appCancel.rawValue,
                    LAError.systemCancel.rawValue, LAError.userFallback.rawValue,
                ]
                if cancelled.contains(code) {
                    throw PWMError(code: "biometric_cancelled")
                }
                throw PWMError(code: "biometric_changed")
            }
            return Redacted(secret as Data)
        }.value
    }

    public func delete() throws {
        var query: [CFString: Any] = [
            kSecClass: kSecClassKey,
            kSecAttrApplicationTag: Data(tag.utf8),
            kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
        ]
        #if os(macOS)
        query[kSecUseDataProtectionKeychain] = true
        #endif
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw PWMError(code: "keychain")
        }
    }
}
#endif
