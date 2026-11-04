import Foundation

/// Touch ID unlock for the browser extension through the Mac app (D48 §5).
/// The extension keeps a sealed blob; the key that opens it is sent here
/// once ("enroll"), kept wrapped by a Secure Enclave key, and handed back
/// after Touch ID ("unlock"). The helper never sees the blob, the vault or
/// the master password. Versioned JSON messages, the same from the native-
/// messaging helper (Chrome, Edge, Brave, Opera, Firefox) and from Safari's
/// extension handler.
public enum BrowserUnlock {
    public static let version = 1
    /// The native-messaging host name; a placeholder until D1.
    public static let hostName = "invalid.pwm.browser"

    public struct Request: Codable, Sendable, Equatable {
        public var v: Int
        /// `enroll`, `unlock` or `forget`.
        public var type: String
        public var accountId: String
        public var deviceId: String
        /// `enroll` only: the 32-byte key, base64.
        public var key: String?

        enum CodingKeys: String, CodingKey {
            case v, type, key
            case accountId = "account_id"
            case deviceId = "device_id"
        }

        public init(v: Int = BrowserUnlock.version, type: String, accountId: String, deviceId: String, key: String? = nil) {
            self.v = v
            self.type = type
            self.accountId = accountId
            self.deviceId = deviceId
            self.key = key
        }
    }

    public struct Reply: Codable, Sendable, Equatable {
        public var v: Int = BrowserUnlock.version
        public var ok: Bool
        /// `unlock` only: the key, base64.
        public var key: String?
        /// `cancelled`, `changed`, `unknown`, `unavailable`, `refused` or `invalid`.
        public var error: String?

        public static let done = Reply(ok: true)
        public static func failed(_ error: String) -> Reply { Reply(ok: false, error: error) }
    }

    /// Who may use the helper: the extension's ids in each store. Placeholders
    /// until the stores give the real ones (M10).
    public static let chromiumOrigins = ["chrome-extension://pppppppppppppppppppppppppppppppp/"]
    public static let firefoxIds = ["pwm@pwm.invalid"]
    /// Safari's messages come only from the extension in this app's bundle.
    public static let safariCaller = "safari"

    public static var callers: Set<String> {
        Set(chromiumOrigins + firefoxIds + [safariCaller])
    }

    /// The caller from the helper's arguments: Chromium browsers pass the
    /// extension's origin first; Firefox passes the manifest's path, then
    /// the extension's id.
    public static func caller(arguments: [String]) -> String? {
        let args = Array(arguments.dropFirst())
        if let first = args.first, first.hasPrefix("chrome-extension://") { return first }
        if args.count >= 2 { return args[1] }
        return nil
    }
}

/// Where the helper keeps each enrolment's wrapped key: the Keychain on
/// macOS, memory in tests.
public protocol BrowserKeyStore: Sendable {
    func get(_ id: String) throws -> String?
    func set(_ id: String, _ value: String) throws
    func delete(_ id: String) throws
}

public final class MemoryBrowserKeyStore: BrowserKeyStore, @unchecked Sendable {
    private var values: [String: String] = [:]
    private let lock = NSLock()

    public init() {}

    public func get(_ id: String) -> String? {
        lock.lock()
        defer { lock.unlock() }
        return values[id]
    }

    public func set(_ id: String, _ value: String) {
        lock.lock()
        defer { lock.unlock() }
        values[id] = value
    }

    public func delete(_ id: String) {
        lock.lock()
        defer { lock.unlock() }
        values.removeValue(forKey: id)
    }

    public var count: Int {
        lock.lock()
        defer { lock.unlock() }
        return values.count
    }
}

/// Answers the extension's requests. Every unlock asks for Touch ID; a
/// changed face or finger deletes the enrolment.
public struct BrowserUnlockHelper: Sendable {
    let callers: Set<String>
    let store: any BrowserKeyStore
    /// The hardware key for one enrolment (its own Secure Enclave key).
    let biometrics: @Sendable (String) -> any BiometricKeyStore

    static let enrollReason = "Allow your browser to unlock PWM with Touch ID"
    static let unlockReason = "Unlock PWM in your browser"

    public init(
        callers: Set<String> = BrowserUnlock.callers,
        store: any BrowserKeyStore,
        biometrics: @escaping @Sendable (String) -> any BiometricKeyStore
    ) {
        self.callers = callers
        self.store = store
        self.biometrics = biometrics
    }

    /// One enrolment per caller, account and extension device.
    static func enrolment(caller: String, account: String, device: String) -> String {
        "\(caller)|\(account)|\(device)"
    }

    /// Ids are the core's (32 lowercase hex characters).
    static func isId(_ text: String) -> Bool {
        text.count == 32 && text.allSatisfy { $0.isHexDigit && !$0.isUppercase }
    }

    public func handle(_ message: Data, caller: String?) async -> Data {
        let reply = await answer(message, caller: caller)
        return (try? JSONEncoder().encode(reply)) ?? Data(#"{"v":1,"ok":false,"error":"invalid"}"#.utf8)
    }

    func answer(_ message: Data, caller: String?) async -> BrowserUnlock.Reply {
        guard let caller, callers.contains(caller) else { return .failed("refused") }
        guard let request = try? JSONDecoder().decode(BrowserUnlock.Request.self, from: message),
              request.v == BrowserUnlock.version,
              Self.isId(request.accountId), Self.isId(request.deviceId)
        else { return .failed("invalid") }
        let id = Self.enrolment(caller: caller, account: request.accountId, device: request.deviceId)
        let hardware = biometrics(id)
        do {
            switch request.type {
            case "enroll":
                guard let text = request.key, let key = Data(base64Encoded: text), key.count == 32 else {
                    return .failed("invalid")
                }
                guard case .available = hardware.availability() else { return .failed("unavailable") }
                let wrapped = try hardware.wrap(Redacted(key))
                // Touch ID once, so turning it on is the user's own act.
                do {
                    _ = try await hardware.unwrap(wrapped, reason: Self.enrollReason)
                } catch {
                    try? hardware.delete()
                    throw error
                }
                try store.set(id, wrapped.base64EncodedString())
                return .done
            case "unlock":
                guard let text = try store.get(id), let wrapped = Data(base64Encoded: text) else {
                    return .failed("unknown")
                }
                let key = try await hardware.unwrap(wrapped, reason: Self.unlockReason)
                return BrowserUnlock.Reply(ok: true, key: key.value.base64EncodedString())
            case "forget":
                try? hardware.delete()
                try store.delete(id)
                return .done
            default:
                return .failed("invalid")
            }
        } catch let failure as PWMError {
            switch failure.code {
            case "biometric_cancelled":
                return .failed("cancelled")
            case "biometric_changed":
                try? hardware.delete()
                try? store.delete(id)
                return .failed("changed")
            case "biometric_unavailable":
                return .failed("unavailable")
            default:
                return .failed("unavailable")
            }
        } catch {
            return .failed("unavailable")
        }
    }
}

/// Native messaging's framing: a 4-byte length in the machine's byte order
/// (little-endian on every Mac), then UTF-8 JSON. Browsers send at most
/// 4 GB and accept at most 1 MB back; the helper takes 64 KB.
public enum NativeMessaging {
    public static let maxIncoming = 64 * 1024

    public static func frame(_ message: Data) -> Data {
        var length = UInt32(message.count).littleEndian
        var out = Data(bytes: &length, count: 4)
        out.append(message)
        return out
    }

    /// The first whole message in `buffer` and what follows it; nil when
    /// more bytes are needed. Throws when a message is too large.
    public static func unframe(_ buffer: Data) throws -> (message: Data, rest: Data)? {
        guard buffer.count >= 4 else { return nil }
        let bytes = [UInt8](buffer.prefix(4))
        let length = Int(UInt32(bytes[0]) | UInt32(bytes[1]) << 8 | UInt32(bytes[2]) << 16 | UInt32(bytes[3]) << 24)
        guard length <= maxIncoming else { throw PWMError(code: "invalid") }
        guard buffer.count >= 4 + length else { return nil }
        let start = buffer.startIndex + 4
        return (Data(buffer[start..<start + length]), Data(buffer[(start + length)...]))
    }
}

/// The native-messaging manifests the Mac app writes when the user turns on
/// "Allow browser extensions to unlock with Touch ID", and removes when it
/// is turned off (D48 §5). Only for browsers whose folder exists.
public struct BrowserHosts: Sendable {
    /// Under `~/Library/Application Support/`: each browser's folder, and
    /// whether it reads Firefox's manifest form.
    public static let browsers: [(folder: String, firefox: Bool)] = [
        ("Google/Chrome", false),
        ("Microsoft Edge", false),
        ("BraveSoftware/Brave-Browser", false),
        ("Chromium", false),
        ("Mozilla", true),
    ]

    public let home: URL
    public let helperPath: String

    public init(home: URL, helperPath: String) {
        self.home = home
        self.helperPath = helperPath
    }

    var support: URL { home.appendingPathComponent("Library/Application Support", isDirectory: true) }

    func file(_ folder: String) -> URL {
        support.appendingPathComponent(folder, isDirectory: true)
            .appendingPathComponent("NativeMessagingHosts", isDirectory: true)
            .appendingPathComponent("\(BrowserUnlock.hostName).json")
    }

    public func manifest(firefox: Bool) throws -> Data {
        var json: [String: Any] = [
            "name": BrowserUnlock.hostName,
            "description": "PWM: unlock with Touch ID",
            "path": helperPath,
            "type": "stdio",
        ]
        if firefox {
            json["allowed_extensions"] = BrowserUnlock.firefoxIds
        } else {
            json["allowed_origins"] = BrowserUnlock.chromiumOrigins
        }
        return try JSONSerialization.data(withJSONObject: json, options: [.prettyPrinted, .sortedKeys])
    }

    /// Writes a manifest for each installed browser; returns their folders.
    @discardableResult
    public func install() throws -> [String] {
        var written: [String] = []
        for browser in Self.browsers {
            let base = support.appendingPathComponent(browser.folder, isDirectory: true)
            guard FileManager.default.fileExists(atPath: base.path) else { continue }
            let target = file(browser.folder)
            do {
                try FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
                try manifest(firefox: browser.firefox).write(to: target, options: .atomic)
            } catch {
                throw PWMError(code: "storage")
            }
            written.append(browser.folder)
        }
        return written
    }

    public func remove() {
        for browser in Self.browsers {
            try? FileManager.default.removeItem(at: file(browser.folder))
        }
    }

    public var isInstalled: Bool {
        Self.browsers.contains { FileManager.default.fileExists(atPath: file($0.folder).path) }
    }
}

#if os(macOS) && canImport(Security) && canImport(LocalAuthentication)
import Security

/// The Keychain as the helper's store: one item per enrolment.
public struct KeychainBrowserKeys: BrowserKeyStore {
    let keychain: KeychainKeyStore

    public init(keychain: KeychainKeyStore) {
        self.keychain = keychain
    }

    public func get(_ id: String) throws -> String? { try keychain.value(account: id) }
    public func set(_ id: String, _ value: String) throws { try keychain.setValue(value, account: id) }
    public func delete(_ id: String) throws { try keychain.deleteValue(account: id) }
}

/// The real helper for the native-messaging helper and Safari's handler: the
/// shared Keychain group from the bundle's Info.plist (`PWMKeychainGroup`),
/// and a Secure Enclave key per enrolment there.
public enum BrowserHelperPlaces {
    public static func helper() -> BrowserUnlockHelper {
        let group = (Bundle.main.object(forInfoDictionaryKey: "PWMKeychainGroup") as? String).flatMap { $0.isEmpty ? nil : $0 }
        var keychain = KeychainKeyStore(service: "pwm.browser", accessGroup: group)
        if !keychain.isReachable() {
            // An unsigned build (CI): no group, and only the login keychain.
            keychain = KeychainKeyStore(service: "pwm.browser", dataProtection: false)
        }
        return BrowserUnlockHelper(store: KeychainBrowserKeys(keychain: keychain)) { id in
            SecureEnclaveKeyStore(tag: "pwm.browser-unlock.\(id)", accessGroup: group)
        }
    }

    /// The user's real home folder: inside the sandbox, `NSHomeDirectory()`
    /// is the app's container, but the browsers read their own folders.
    public static var realHome: URL {
        if let entry = getpwuid(getuid()), let dir = entry.pointee.pw_dir {
            return URL(fileURLWithPath: String(cString: dir), isDirectory: true)
        }
        return FileManager.default.homeDirectoryForCurrentUser
    }
}
#endif
