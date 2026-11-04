import Foundation
import Observation
import PWMKit

/// Reading and deleting the file the user picked; the real one uses the
/// system's security-scoped access, tests a fake.
public protocol ImportFiles: Sendable {
    /// The file's size in bytes, if the system knows it.
    func size(of url: URL) -> Int?
    func read(_ url: URL) throws -> Data
    /// Removes the file outright (it doesn't go to the Trash).
    func delete(_ url: URL) throws
}

public struct SystemImportFiles: ImportFiles {
    public init() {}

    public func size(of url: URL) -> Int? {
        scoped(url) { (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize }
    }

    public func read(_ url: URL) throws -> Data {
        try scoped(url) { try Data(contentsOf: url) }
    }

    public func delete(_ url: URL) throws {
        try scoped(url) { try FileManager.default.removeItem(at: url) }
    }

    private func scoped<T>(_ url: URL, _ work: () throws -> T) rethrows -> T {
        #if canImport(Darwin)
        let started = url.startAccessingSecurityScopedResource()
        defer { if started { url.stopAccessingSecurityScopedResource() } }
        #endif
        return try work()
    }
}

/// Import in Settings (D51 §9): pick a file, check where it's from, enter
/// its password if it has one, see what will come in, then import. After a
/// file with no password, one line says it holds passwords in plain text,
/// with a button to delete it. No master password: importing only adds.
@MainActor
@Observable
public final class ImportModel {
    public private(set) var fileName: String?
    public var format = "" {
        didSet { if format != oldValue { summary = nil } }
    }
    public var password = ""
    /// A KeePass key file's name, once picked.
    public private(set) var keyFileName: String?
    private var keyFile: Redacted<Data>?
    public private(set) var needsPassword = false
    /// For a generic CSV.
    public private(set) var columns: CSVColumns?
    public var columnMap = ColumnMap() {
        didSet { if columnMap != oldValue { summary = nil } }
    }
    /// What "Import" will bring in.
    public private(set) var summary: ImportSummary?
    /// What came in.
    public private(set) var done: ImportSummary?
    /// The imported file had no password: it holds passwords in plain text.
    public private(set) var plaintext = false
    public private(set) var deleted = false
    public private(set) var working = false
    public private(set) var error: String?

    private var data: Redacted<Data>?
    private var url: URL?
    private let account: any Account
    private let files: any ImportFiles

    init(account: any Account, files: any ImportFiles) {
        self.account = account
        self.files = files
    }

    /// The password field shows for encrypted exports and the formats that
    /// may have one.
    public var asksPassword: Bool {
        needsPassword || ["bitwarden", "keepass", "pwm"].contains(format)
    }

    /// KeePass databases may also need a key file.
    public var asksKeyFile: Bool { format == "keepass" }

    /// The user picked a KeePass key file (at most 1 MB).
    public func pickedKeyFile(_ url: URL) {
        summary = nil
        guard let size = files.size(of: url), size <= 1024 * 1024, let data = try? files.read(url) else {
            error = PWMError(code: "import_unreadable").message
            return
        }
        keyFile = Redacted(data)
        keyFileName = url.lastPathComponent
    }

    public var canCheck: Bool { data != nil && !format.isEmpty && !working }

    /// The user picked a file: it is read into memory only.
    public func picked(_ url: URL) async {
        reset()
        if let size = files.size(of: url), size > ImportFile.maxBytes {
            error = PWMError(code: "import_too_large").message
            return
        }
        do {
            let data = Redacted(try files.read(url))
            guard data.value.count <= ImportFile.maxBytes else {
                error = PWMError(code: "import_too_large").message
                return
            }
            self.data = data
            self.url = url
            fileName = url.lastPathComponent
            let guess = await account.guessImport(fileName: url.lastPathComponent, data: data)
            format = guess?.format ?? ""
            needsPassword = guess?.needsPassword ?? false
            if format == "csv" {
                let found = try await account.importColumns(data)
                columns = found
                columnMap = found.suggested
            }
            summary = nil
        } catch {
            self.error = message(for: error)
        }
    }

    /// Reads the file as the chosen format and shows what will come in.
    public func check() async {
        guard let data, canCheck else { return }
        working = true
        defer { working = false }
        error = nil
        do {
            summary = try await account.prepareImport(
                format: format, data: data,
                password: password.isEmpty ? nil : Redacted(password),
                keyFile: format == "keepass" ? keyFile : nil,
                columns: format == "csv" ? columnMap : nil
            )
        } catch {
            summary = nil
            self.error = message(for: error)
        }
    }

    /// Saves what `check` showed; the file's bytes are dropped.
    public func importNow() async {
        guard summary != nil, !working else { return }
        working = true
        defer { working = false }
        error = nil
        do {
            done = try await account.finishImport()
            plaintext = password.isEmpty && keyFile == nil
            summary = nil
            data = nil
            password = ""
            keyFile = nil
            keyFileName = nil
        } catch {
            self.error = message(for: error)
        }
    }

    /// Deletes the picked export file; offered after a plaintext import.
    public func deleteFile() {
        guard let url else { return }
        do {
            try files.delete(url)
            deleted = true
            self.url = nil
        } catch {
            self.error = PWMError(code: "delete_failed").message
        }
    }

    public var canDelete: Bool { plaintext && url != nil && !deleted }

    /// Leaving the screen drops the file and any preview.
    public func close() async {
        if summary != nil { await account.cancelImport() }
        data = nil
        keyFile = nil
        password = ""
        summary = nil
    }

    private func reset() {
        data = nil
        url = nil
        fileName = nil
        format = ""
        password = ""
        keyFile = nil
        keyFileName = nil
        needsPassword = false
        columns = nil
        columnMap = ColumnMap()
        summary = nil
        done = nil
        plaintext = false
        deleted = false
        error = nil
    }

    // MARK: Labels

    /// "2 logins, 1 card".
    public static func counts(_ s: ImportSummary) -> String {
        let parts: [(Int, String, String)] = [
            (s.logins, "login", "logins"), (s.notes, "note", "notes"), (s.cards, "card", "cards"),
            (s.identities, "identity", "identities"), (s.folders, "folder", "folders"),
        ]
        let listed = parts.filter { $0.0 > 0 }.map { "\($0.0) \($0.0 == 1 ? $0.1 : $0.2)" }
        return listed.isEmpty ? "nothing new" : listed.joined(separator: ", ")
    }

    public static func leftOut(_ l: ImportSummary.LeftOut) -> String {
        let what = [
            "attachments": "attachments and documents",
            "ssh_keys": "SSH keys",
            "aliases": "email aliases",
            "passkeys": "passkeys this export can't carry",
            "unknown_types": "items of a type PWM doesn't have",
            "empty": "empty rows",
            "too_large": "items too large to sync",
            "duplicates": "items already in your vault",
        ][l.reason] ?? l.reason
        return "Left out: \(l.count) \(what)"
    }

    /// The fields the column picker offers.
    public static let mappedFields: [(label: String, keyPath: WritableKeyPath<ColumnMap, Int?>)] = [
        ("Title", \.title), ("Website", \.url), ("Username", \.username), ("Password", \.password),
        ("Notes", \.notes), ("One-time code", \.totp), ("Folder", \.folder),
    ]
}
