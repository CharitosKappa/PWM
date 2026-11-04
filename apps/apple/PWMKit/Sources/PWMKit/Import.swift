import Foundation
@preconcurrency import PWMCore

/// Importing another manager's export, or our own (D51). The core reads the
/// file in memory; these are its answers as Swift values.
public enum ImportFile {
    /// Formats the core reads, with the names it uses.
    public static let formats: [(id: String, label: String)] = [
        ("bitwarden", "Bitwarden (.json)"),
        ("1password", "1Password (.1pux)"),
        ("keeper", "Keeper (.json)"),
        ("keepass", "KeePass (.kdbx)"),
        ("nordpass", "NordPass (.csv)"),
        ("protonpass", "Proton Pass (.zip or .json)"),
        ("dashlane", "Dashlane (.zip, .csv or .json)"),
        ("lastpass", "LastPass (.csv)"),
        ("csv", "Other CSV, such as a browser's"),
        ("pwm", "PWM export (.pwmx, .json or .csv)"),
    ]

    /// The core's own limit (D51 §7).
    public static let maxBytes = 50 * 1024 * 1024
}

/// What a picked file looks like.
public struct ImportGuess: Sendable, Equatable {
    public let format: String
    public let needsPassword: Bool

    public init(format: String, needsPassword: Bool) {
        self.format = format
        self.needsPassword = needsPassword
    }
}

/// Which CSV column (by index) holds each field; nil leaves it out.
public struct ColumnMap: Sendable, Equatable {
    public var title: Int?
    public var url: Int?
    public var username: Int?
    public var password: Int?
    public var notes: Int?
    public var totp: Int?
    public var folder: Int?
    public var favorite: Int?
    public var tags: Int?

    public init(
        title: Int? = nil, url: Int? = nil, username: Int? = nil, password: Int? = nil, notes: Int? = nil,
        totp: Int? = nil, folder: Int? = nil, favorite: Int? = nil, tags: Int? = nil
    ) {
        self.title = title
        self.url = url
        self.username = username
        self.password = password
        self.notes = notes
        self.totp = totp
        self.folder = folder
        self.favorite = favorite
        self.tags = tags
    }

    init(_ m: ImportMapping) {
        func i(_ v: UInt32?) -> Int? { v.map(Int.init) }
        self.init(
            title: i(m.title), url: i(m.url), username: i(m.username), password: i(m.password),
            notes: i(m.notes), totp: i(m.totp), folder: i(m.folder), favorite: i(m.favorite), tags: i(m.tags)
        )
    }

    var core: ImportMapping {
        func u(_ v: Int?) -> UInt32? { v.flatMap { UInt32(exactly: $0) } }
        return ImportMapping(
            title: u(title), url: u(url), username: u(username), password: u(password), notes: u(notes),
            totp: u(totp), folder: u(folder), favorite: u(favorite), tags: u(tags)
        )
    }
}

/// A CSV's header row, with the suggested columns.
public struct CSVColumns: Sendable, Equatable {
    public let headers: [String]
    public let suggested: ColumnMap
    public let rows: Int

    public init(headers: [String], suggested: ColumnMap, rows: Int) {
        self.headers = headers
        self.suggested = suggested
        self.rows = rows
    }
}

/// What an import brings in, and what it leaves out.
public struct ImportSummary: Sendable, Equatable {
    public struct LeftOut: Sendable, Equatable {
        /// `attachments`, `ssh_keys`, `aliases`, `passkeys`, `unknown_types`,
        /// `empty`, `too_large` or `duplicates`.
        public let reason: String
        public let count: Int

        public init(reason: String, count: Int) {
            self.reason = reason
            self.count = count
        }
    }

    public let logins: Int
    public let notes: Int
    public let cards: Int
    public let identities: Int
    public let folders: Int
    /// Of the above, how many come in the trash.
    public let trashed: Int
    public let leftOut: [LeftOut]

    public init(logins: Int, notes: Int, cards: Int, identities: Int, folders: Int, trashed: Int, leftOut: [LeftOut]) {
        self.logins = logins
        self.notes = notes
        self.cards = cards
        self.identities = identities
        self.folders = folders
        self.trashed = trashed
        self.leftOut = leftOut
    }

    init(_ p: ImportPreview) {
        self.init(
            logins: Int(p.logins), notes: Int(p.notes), cards: Int(p.cards), identities: Int(p.identities),
            folders: Int(p.folders), trashed: Int(p.trashed),
            leftOut: p.leftOut.map { LeftOut(reason: $0.reason, count: Int($0.count)) }
        )
    }
}

extension Session {
    /// The format a picked file looks like, or nil.
    public nonisolated func guessImport(fileName: String, data: Redacted<Data>) -> ImportGuess? {
        PWMCore.detectImport(fileName: fileName, bytes: data.value).map {
            ImportGuess(format: $0.format, needsPassword: $0.needsPassword)
        }
    }

    /// A CSV's header and the suggested columns, for the column picker.
    public nonisolated func importColumns(_ data: Redacted<Data>) throws -> CSVColumns {
        let columns = try core { try PWMCore.importColumns(bytes: data.value) }
        return CSVColumns(headers: columns.headers, suggested: ColumnMap(columns.suggested), rows: Int(columns.rows))
    }

    /// Reads the file into the first vault's preview; nothing is saved yet.
    /// `keyFile` is a KeePass key file.
    public func prepareImport(
        format: String, data: Redacted<Data>, password: Redacted<String>?, keyFile: Redacted<Data>?,
        columns: ColumnMap?
    ) throws -> ImportSummary {
        let preview = try core {
            try client.prepareImport(
                vaultId: nil, format: format, bytes: data.value, password: password?.value,
                keyFile: keyFile?.value, mapping: columns?.core, nowMs: now()
            )
        }
        return ImportSummary(preview)
    }

    /// Saves the prepared import, then syncs it up (a failed sync leaves it
    /// queued, like any change).
    public func finishImport() async throws -> ImportSummary {
        let done = ImportSummary(try core { try client.finishImport(nowMs: now()) })
        refreshSuggestions()
        _ = try? await sync()
        return done
    }

    public func cancelImport() {
        client.cancelImport()
    }
}
