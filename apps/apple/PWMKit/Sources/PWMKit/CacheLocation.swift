import Foundation

/// Where the cache file lives (D41 §4). It holds ciphertext only (D20).
public enum CacheLocation {
    static let folder = "PWM"
    static let file = "vault.sqlite"

    /// The cache in a folder of `directory`, created if needed.
    public static func path(in directory: URL) throws -> String {
        let folder = directory.appendingPathComponent(folder, isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: protection)
        } catch {
            throw PWMError(code: "storage")
        }
        return folder.appendingPathComponent(file).path
    }

    #if os(iOS) || os(macOS)
    /// The App Group container shared with the AutoFill extension (M5.5).
    public static func appGroup(_ identifier: String) throws -> String {
        guard let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: identifier) else {
            throw PWMError(code: "storage")
        }
        return try path(in: container)
    }
    #endif

    /// Readable after the first unlock, so AutoFill and background sync
    /// (M5.8) work while the device is locked. Files SQLite creates in the
    /// folder take the app's default class; the app sets it to the same in
    /// its entitlements (M5.2).
    static var protection: [FileAttributeKey: Any]? {
        #if os(iOS)
        [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
        #else
        nil
        #endif
    }
}
