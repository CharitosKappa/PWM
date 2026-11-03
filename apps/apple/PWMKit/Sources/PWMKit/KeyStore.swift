import Foundation

/// What this device keeps outside the cache (D20 §2): the Secret Key, the
/// session tokens and the device id. The core decides what they are;
/// Swift only keeps the bytes.
public enum KeyName: String, Sendable, CaseIterable {
    case secretKey = "secret-key"
    case accessToken = "access-token"
    case refreshToken = "refresh-token"
    case deviceId = "device-id"
    case email
    /// Face ID / Touch ID unlock (D45): the core's blob, the unlock key
    /// wrapped by the Secure Enclave key (both base64), and when the master
    /// password was last typed (Unix ms).
    case biometricBlob = "biometric-blob"
    case biometricKey = "biometric-key"
    case passwordUnlockedAt = "password-unlocked-at"
}

/// Secure storage for `KeyName` values: the Keychain on Apple platforms,
/// memory in tests.
public protocol KeyStore: Sendable {
    func get(_ name: KeyName) throws -> String?
    func set(_ name: KeyName, _ value: String) throws
    func delete(_ name: KeyName) throws
}

/// Keeps values in memory only: tests and previews.
public final class MemoryKeyStore: KeyStore, @unchecked Sendable {
    private var values: [KeyName: String] = [:]
    private let lock = NSLock()

    public init() {}

    public func get(_ name: KeyName) -> String? {
        lock.lock()
        defer { lock.unlock() }
        return values[name]
    }

    public func set(_ name: KeyName, _ value: String) {
        lock.lock()
        defer { lock.unlock() }
        values[name] = value
    }

    public func delete(_ name: KeyName) {
        lock.lock()
        defer { lock.unlock() }
        values.removeValue(forKey: name)
    }
}

#if canImport(Security)
import os
import Security

/// Only the status of a refused Keychain call, and which key: never a value.
private let keychainLog = Logger(subsystem: "pwm", category: "keychain")

private func refused(_ status: OSStatus, _ call: String, _ name: String) -> PWMError {
    keychainLog.error("\(call, privacy: .public) \(name, privacy: .public) refused: \(status, privacy: .public)")
    return PWMError(code: "keychain")
}

/// The Keychain: generic passwords, this device only, readable after the
/// first unlock (so AutoFill and background sync work), optionally in an
/// access group shared with the AutoFill extension (D41 §4).
public struct KeychainKeyStore: KeyStore {
    public let service: String
    public let accessGroup: String?
    /// The data-protection Keychain, as on iOS. A Mac build signed without
    /// a Keychain entitlement (CI's ad-hoc builds) can only use the login
    /// keychain, which `SharedStorage` then picks (D48 §2).
    public let dataProtection: Bool

    public init(service: String = "pwm", accessGroup: String? = nil, dataProtection: Bool = true) {
        self.service = service
        self.accessGroup = accessGroup
        self.dataProtection = dataProtection
    }

    private func query(_ name: KeyName) -> [CFString: Any] {
        query(account: name.rawValue)
    }

    private func query(account: String) -> [CFString: Any] {
        var query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
        ]
        if dataProtection {
            query[kSecUseDataProtectionKeychain] = true
        }
        if let accessGroup {
            query[kSecAttrAccessGroup] = accessGroup
        }
        return query
    }

    public func get(_ name: KeyName) throws -> String? {
        var query = query(name)
        query[kSecReturnData] = true
        query[kSecMatchLimit] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else {
            throw refused(status, "get", name.rawValue)
        }
        return String(data: data, encoding: .utf8)
    }

    public func set(_ name: KeyName, _ value: String) throws {
        let data = Data(value.utf8)
        var update: [CFString: Any] = [kSecValueData: data]
        if dataProtection {
            update[kSecAttrAccessible] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        }
        var status = SecItemUpdate(query(name) as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            let add = query(name).merging(update) { $1 }
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw refused(status, "set", name.rawValue) }
    }

    public func delete(_ name: KeyName) throws {
        let status = SecItemDelete(query(name) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw refused(status, "delete", name.rawValue)
        }
    }

    /// Whether this build may store items here: a build signed without the
    /// access group (or, on macOS, without any Keychain entitlement) is
    /// refused. Reading alone can answer "not found" where writing is
    /// refused, so this writes and deletes a probe item, which holds nothing.
    public func isReachable() -> Bool {
        let probe = query(account: "reachability-probe")
        SecItemDelete(probe as CFDictionary)
        var add = probe
        add[kSecValueData] = Data()
        if dataProtection {
            add[kSecAttrAccessible] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        }
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess else {
            keychainLog.info("probe refused: \(status, privacy: .public), data protection \(dataProtection, privacy: .public)")
            return false
        }
        SecItemDelete(probe as CFDictionary)
        return true
    }
}
#endif
