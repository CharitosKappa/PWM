import Foundation

/// The one-time move into the places the app shares with its AutoFill
/// extension (D47 §2): the cache files into the App Group container, the
/// keys into the shared Keychain group. Each step copies (or moves) first
/// and deletes after, so a run stopped halfway is finished by the next one.
/// It runs before the core opens the cache.
public enum StorageMigration {
    /// SQLite's files for the cache; the main file moves last, so while it
    /// is still in the old place the move has not finished.
    static let suffixes = ["-wal", "-shm", ""]

    public static func run(
        oldCache: String,
        newCache: String,
        oldKeys: any KeyStore,
        newKeys: any KeyStore,
        oldBiometrics: any BiometricKeyStore,
        files: any FileMover = SystemFileMover()
    ) throws {
        try moveCache(from: oldCache, to: newCache, files: files)
        try moveKeys(from: oldKeys, to: newKeys, oldBiometrics: oldBiometrics)
    }

    static func moveCache(from old: String, to new: String, files: any FileMover) throws {
        guard old != new, files.exists(old) else { return }
        for suffix in suffixes {
            let (source, target) = (old + suffix, new + suffix)
            guard files.exists(source) else { continue }
            // Anything there is from a run stopped halfway: the old main
            // file is still in place.
            if files.exists(target) { try files.remove(target) }
            try files.move(source, to: target)
        }
    }

    static func moveKeys(from old: any KeyStore, to new: any KeyStore, oldBiometrics: any BiometricKeyStore) throws {
        for name in KeyName.allCases {
            guard let value = try old.get(name) else { continue }
            if name == .biometricKey {
                // Wrapped by a Secure Enclave key the extension can't
                // reach: it is dropped, and the next master password unlock
                // makes both again in the shared group (D47 §2).
                try oldBiometrics.delete()
                try old.delete(name)
                continue
            }
            if try new.get(name) == nil {
                try new.set(name, value)
            }
            try old.delete(name)
        }
    }
}

/// The file operations the migration needs; a fake in tests.
public protocol FileMover: Sendable {
    func exists(_ path: String) -> Bool
    func move(_ path: String, to target: String) throws
    func remove(_ path: String) throws
}

public struct SystemFileMover: FileMover {
    public init() {}

    public func exists(_ path: String) -> Bool {
        FileManager.default.fileExists(atPath: path)
    }

    public func move(_ path: String, to target: String) throws {
        do {
            try FileManager.default.moveItem(atPath: path, toPath: target)
        } catch {
            throw PWMError(code: "storage")
        }
    }

    public func remove(_ path: String) throws {
        do {
            try FileManager.default.removeItem(atPath: path)
        } catch {
            throw PWMError(code: "storage")
        }
    }
}

/// Where a `Session` keeps its cache and keys.
public struct SessionPlaces: Sendable {
    public let cachePath: String
    public let keys: any KeyStore
    public let biometrics: any BiometricKeyStore
    /// Whether the AutoFill extension can reach them.
    public let shared: Bool
}

#if canImport(Security) && (os(iOS) || os(macOS))
import Security

/// The App Group and Keychain group the app and the extension share
/// (D47 §1), read from the bundle's Info.plist (`PWMAppGroup`,
/// `PWMKeychainGroup`) so the team prefix comes from the build.
public enum SharedStorage {
    static var appGroup: String? { Bundle.main.object(forInfoDictionaryKey: "PWMAppGroup") as? String }
    static var keychainGroup: String? {
        (Bundle.main.object(forInfoDictionaryKey: "PWMKeychainGroup") as? String).flatMap { $0.isEmpty ? nil : $0 }
    }

    /// The app's places. The first launch with the shared places moves
    /// everything there; a build that can't reach them (no App Group or
    /// Keychain group in its signature) keeps the app's own, and the
    /// extension then says to open the app.
    public static func app() throws -> SessionPlaces {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )
        let own = SessionPlaces(
            cachePath: try CacheLocation.path(in: support),
            keys: KeychainKeyStore(),
            biometrics: Biometrics.platform,
            shared: false
        )
        guard let shared = try? sharedPlaces() else { return own }
        try StorageMigration.run(
            oldCache: own.cachePath, newCache: shared.cachePath,
            oldKeys: own.keys, newKeys: shared.keys, oldBiometrics: own.biometrics
        )
        return shared
    }

    /// The extension's places: the shared ones only.
    public static func autoFill() throws -> SessionPlaces {
        do {
            return try sharedPlaces()
        } catch {
            throw PWMError(code: "not_signed_in")
        }
    }

    static func sharedPlaces() throws -> SessionPlaces {
        guard let appGroup, let keychainGroup else { throw PWMError(code: "storage") }
        let keys = KeychainKeyStore(accessGroup: keychainGroup)
        guard keys.isReachable() else { throw PWMError(code: "keychain") }
        #if canImport(LocalAuthentication)
        let biometrics: any BiometricKeyStore = SecureEnclaveKeyStore(accessGroup: keychainGroup)
        #else
        let biometrics: any BiometricKeyStore = NoBiometrics()
        #endif
        return SessionPlaces(
            cachePath: try CacheLocation.appGroup(appGroup),
            keys: keys,
            biometrics: biometrics,
            shared: true
        )
    }
}
#endif
