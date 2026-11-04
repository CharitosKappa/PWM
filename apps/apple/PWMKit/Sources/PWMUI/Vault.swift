import Foundation
import Observation
import PWMKit

// The vault screens' state (D43 §2–4): the list with its sections and
// search, an item's detail, and editing. Items are opened by the core and
// held here only while the vault is unlocked; locking drops these models.

/// An item and the vault it lives in.
public struct Entry: Identifiable, Sendable, Equatable {
    public let vaultId: String
    public var item: Item

    public init(vaultId: String, item: Item) {
        self.vaultId = vaultId
        self.item = item
    }

    public var id: String { item.id ?? "" }
}

/// The vault list: sections, search, sync and the actions on items.
@MainActor
@Observable
public final class VaultModel {
    public enum Section: Hashable, Sendable {
        case all, favorites, kind(Item.Kind), folder(String), trash
    }

    /// The type sections, in sidebar order.
    public static let kinds: [Item.Kind] = [.login, .card, .identity, .note]

    public var section: Section = .all
    public var query = ""
    /// A new item asked for from the menu bar (⌘N on macOS); the list
    /// opens the editor for it.
    public var pendingNew: Item.Kind?

    public func requestNew(_ kind: Item.Kind) {
        if section == .trash { section = .all }
        pendingNew = kind
    }
    public private(set) var entries: [Entry] = []
    public private(set) var loaded = false
    public private(set) var syncing = false
    /// A sync notice, such as "Couldn't sync"; the vault still shows.
    public private(set) var status: String?
    public private(set) var error: String?
    /// "Sign in again to sync" (D50 §5); set by the app.
    public internal(set) var signInAgain: SignInAgainModel?

    let account: any Account
    let pasteboard: any SecretPasteboard
    let capture: ScreenCapture
    private let onSignedOut: @MainActor () -> Void
    private var syncAgain = false

    init(
        account: any Account, pasteboard: any SecretPasteboard, capture: ScreenCapture = .shared,
        onSignedOut: @escaping @MainActor () -> Void
    ) {
        self.account = account
        self.pasteboard = pasteboard
        self.capture = capture
        self.onSignedOut = onSignedOut
    }

    /// The items the current section and search show, in order.
    public var visible: [Entry] { Self.filter(entries, section: section, query: query) }

    /// Folders not in the trash, by name.
    public var folders: [Entry] {
        entries.filter { $0.item.kind == .folder && $0.item.trashedAtMs == nil }.sorted(by: Self.order)
    }

    public func entry(_ id: String?) -> Entry? {
        guard let id else { return nil }
        return entries.first { $0.id == id }
    }

    // MARK: Search (D43 §3, the same rule on every platform)

    static func filter(_ entries: [Entry], section: Section, query: String) -> [Entry] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return entries
            .filter { $0.item.kind != .folder && inSection($0.item, section) }
            .filter { needle.isEmpty || matches($0.item, needle) }
            .sorted(by: order)
    }

    static func inSection(_ item: Item, _ section: Section) -> Bool {
        let trashed = item.trashedAtMs != nil
        switch section {
        case .trash: return trashed
        case .all: return !trashed
        case .favorites: return !trashed && item.favorite
        case let .kind(kind): return !trashed && item.kind == kind
        case let .folder(id): return !trashed && item.folderId == id
        }
    }

    /// A case-insensitive substring of the title, username, a URL or a
    /// card's last 4 digits.
    static func matches(_ item: Item, _ needle: String) -> Bool {
        var fields = [item.title]
        if let login = item.login {
            fields.append(login.username)
            fields += login.urls.map(\.url)
        }
        if let card = item.card, !card.lastFour.isEmpty {
            fields.append(card.lastFour)
        }
        return fields.contains { $0.range(of: needle, options: [.caseInsensitive]) != nil }
    }

    /// By title in the user's language, then by id, so the order is stable.
    static func order(_ a: Entry, _ b: Entry) -> Bool {
        switch a.item.title.localizedStandardCompare(b.item.title) {
        case .orderedAscending: true
        case .orderedDescending: false
        case .orderedSame: a.id < b.id
        }
    }

    // MARK: Loading and sync

    /// Reads every vault from the cache on this device.
    public func load() async {
        do {
            var all: [Entry] = []
            for vault in try await account.vaultIds() {
                all += try await account.items(in: vault).map { Entry(vaultId: vault, item: $0) }
            }
            entries = all
            loaded = true
            error = nil
        } catch {
            self.error = message(for: error)
        }
    }

    /// Syncs, empties old trash and reloads. A sync already running runs
    /// once more when it ends, so no change waits for the next refresh.
    public func refresh() async {
        guard !syncing else {
            syncAgain = true
            return
        }
        syncing = true
        defer { syncing = false }
        repeat {
            syncAgain = false
            await syncOnce()
        } while syncAgain
    }

    private func syncOnce() async {
        do {
            switch try await account.sync() {
            case let .synced(_, _, _, failed):
                status = failed > 0 ? "Some changes couldn't sync. They'll be sent again." : nil
            case .retry:
                status = "Couldn't sync. Showing what's on this device."
            case .signedOut:
                if let signInAgain {
                    signInAgain.sessionEnded()
                    status = nil
                } else {
                    status = PWMError(code: "signed_out").message
                }
            case .deviceRevoked:
                // The core has cleared the cache; finish signing out.
                try? await account.signOut()
                onSignedOut()
                return
            }
            _ = try await account.purgeTrash()
        } catch {
            status = message(for: error)
        }
        await load()
    }

    // MARK: Actions

    /// The vault new items go to: the first, as in the extension.
    func defaultVault() async throws -> String {
        guard let vault = try await account.vaultIds().first else { throw PWMError(code: "unknown_vault") }
        return vault
    }

    public func detail(_ entry: Entry) -> ItemDetailModel {
        ItemDetailModel(entry: entry, account: account, pasteboard: pasteboard, capture: capture)
    }

    public func edit(_ entry: Entry) -> ItemEditModel {
        ItemEditModel(entry: entry, isNew: false, vault: self)
    }

    /// A new item of `kind`, not saved until the user saves it. New items go
    /// into the folder being shown, and favorites when that is shown.
    public func newItem(_ kind: Item.Kind) async -> ItemEditModel? {
        do {
            var item = try await account.newItem(kind, title: "")
            if case let .folder(id) = section { item.folderId = id }
            if section == .favorites { item.favorite = true }
            return ItemEditModel(entry: Entry(vaultId: try await defaultVault(), item: item), isNew: true, vault: self)
        } catch {
            self.error = message(for: error)
            return nil
        }
    }

    /// After a save: show it now, sync in the background.
    func saved() async {
        await load()
        Task { await refresh() }
    }

    public func trash(_ entry: Entry) async {
        await change { try await self.account.setTrashed(entry.id, in: entry.vaultId, trashed: true) }
    }

    public func restore(_ entry: Entry) async {
        await change { try await self.account.setTrashed(entry.id, in: entry.vaultId, trashed: false) }
    }

    /// Deletes a trashed item for good; the screen asks first.
    public func deleteForever(_ entry: Entry) async {
        await change { try await self.account.delete(entry.id, in: entry.vaultId) }
    }

    private func change(_ action: @escaping () async throws -> Void) async {
        do {
            try await action()
            error = nil
            await saved()
        } catch {
            self.error = message(for: error)
        }
    }
}

/// One item's fields: hidden values stay masked until tapped, and are
/// hidden while the screen is captured (D49).
@MainActor
@Observable
public final class ItemDetailModel: CaptureListener {
    public struct Code: Sendable, Equatable {
        public let value: Redacted<String>
        public let secondsLeft: UInt64
    }

    public let entry: Entry
    /// Keys of the hidden fields the user revealed: `password`, `number`,
    /// `securityCode`, `field.<n>`, `history.<n>`.
    public private(set) var revealed: Set<String> = []
    public private(set) var code: Code?
    /// The login's code is a type this version can't make (HOTP, say).
    public private(set) var codeUnsupported = false
    /// The field copied last, for its "Copied" label.
    public private(set) var copied: String?
    /// After a screenshot masked what was shown (D49 §3).
    public private(set) var notice: String?

    /// As for the Secret Key (D42).
    public static let clipboardSeconds: TimeInterval = 30

    private let account: any Account
    private let pasteboard: any SecretPasteboard
    private let capture: ScreenCapture

    init(entry: Entry, account: any Account, pasteboard: any SecretPasteboard, capture: ScreenCapture = .shared) {
        self.entry = entry
        self.account = account
        self.pasteboard = pasteboard
        self.capture = capture
        capture.add(self)
    }

    public var item: Item { entry.item }

    /// Hidden fields and the one-time code show `ScreenCapture.hiddenLine`
    /// instead; copying still works.
    public var hiddenByCapture: Bool { capture.isRecorded }

    public func isRevealed(_ field: String) -> Bool { !capture.isRecorded && revealed.contains(field) }

    /// Nothing is revealed while the screen is captured.
    public func toggle(_ field: String) {
        guard !capture.isRecorded else { return }
        notice = nil
        if revealed.contains(field) { revealed.remove(field) } else { revealed.insert(field) }
    }

    // Revealed values don't come back by themselves after a capture.
    func captureStarted() {
        revealed = []
    }

    func screenshotTaken() {
        guard !revealed.isEmpty else { return }
        revealed = []
        notice = ScreenCapture.screenshotLine
    }

    /// Masks everything again, when the screen goes away.
    public func hideAll() {
        revealed = []
        code = nil
    }

    public func copy(_ value: String, field: String) {
        pasteboard.copy(Redacted(value), clearAfter: Self.clipboardSeconds)
        copied = field
    }

    public func copyCode() {
        guard let code else { return }
        copy(code.value.value, field: "code")
    }

    /// Reads the current one-time code; the screen calls it every second.
    public func refreshCode() async {
        guard let totp = item.login?.totp, !totp.isEmpty else { return }
        do {
            code = try await account.currentCode(entry.id, in: entry.vaultId).map {
                Code(value: $0.code, secondsLeft: $0.secondsLeft)
            }
            codeUnsupported = code == nil
        } catch {
            code = nil
            codeUnsupported = true
        }
    }

    /// Display text for a field: `•` for each hidden character, up to 12.
    public static func masked(_ value: String) -> String {
        String(repeating: "•", count: min(max(value.count, 8), 12))
    }
}

/// A new or edited item. Fields are edited in place on copies of the item's
/// objects, so fields a newer app wrote stay as they are (D18 §2).
@MainActor
@Observable
public final class ItemEditModel {
    public let isNew: Bool
    public let vaultId: String
    public var title: String
    public var notes: String
    public var favorite: Bool
    public var folderId: String?
    /// Comma-separated.
    public var tags: String
    public var login: Login
    /// The one-time code as typed or scanned; checked by the core on save.
    public var totp: String
    public var card: Card
    public var expiryMonth: String
    public var expiryYear: String
    public var identity: Identity
    /// One address line per line.
    public var address: String
    /// The login's websites; rows have ids so lists can add and delete them.
    public var urls: [Row<LoginURL>]
    public var customFields: [Row<CustomField>]
    public private(set) var folders: [Entry]
    public private(set) var busy = false
    public private(set) var error: String?

    private let original: Item
    private let vault: VaultModel

    init(entry: Entry, isNew: Bool, vault: VaultModel) {
        let item = entry.item
        self.isNew = isNew
        self.vaultId = entry.vaultId
        self.vault = vault
        original = item
        folders = vault.folders
        title = item.title
        notes = item.notes
        favorite = item.favorite
        folderId = item.folderId
        tags = item.tags.joined(separator: ", ")
        login = item.login ?? Login()
        totp = item.login?.totp ?? ""
        card = item.card ?? Card()
        expiryMonth = item.card?.expiryMonth.map(String.init) ?? ""
        expiryYear = item.card?.expiryYear.map(String.init) ?? ""
        identity = item.identity ?? Identity()
        address = item.identity?.addressLines.joined(separator: "\n") ?? ""
        urls = (item.login?.urls ?? []).map(Row.init)
        customFields = item.customFields.map(Row.init)
    }

    /// A list row with a stable id.
    public struct Row<Value: Sendable & Equatable>: Identifiable, Sendable, Equatable {
        public let id = UUID()
        public var value: Value

        init(_ value: Value) {
            self.value = value
        }
    }

    public var kind: Item.Kind? { original.kind }
    public var isEditable: Bool { original.isEditable }

    /// Whether leaving should ask to discard.
    public var hasChanges: Bool {
        guard let edited = try? assembled(checkingCode: false) else { return true }
        return edited != original
    }

    public var canSave: Bool {
        !busy && isEditable && !title.trimmingCharacters(in: .whitespaces).isEmpty && (isNew || hasChanges)
    }

    public func addURL() {
        urls.append(Row(LoginURL(url: "")))
    }

    public func addField(_ kind: CustomField.Kind) {
        customFields.append(Row(CustomField(name: "", value: "", kind: kind)))
    }

    /// A generator for the password field's sheet.
    func makeGenerator() -> GeneratorModel {
        GeneratorModel(pasteboard: vault.pasteboard, capture: vault.capture)
    }

    /// A password from the generator sheet.
    public func use(_ generated: Redacted<String>) {
        login.password = generated.value
    }

    /// Makes a folder now (it is saved at once) and puts the item in it.
    public func newFolder(named name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        do {
            let folder = try await vault.account.newItem(.folder, title: name)
            let id = try await vault.account.save(folder, in: vaultId)
            await vault.load()
            folders = vault.folders
            folderId = id
        } catch {
            self.error = message(for: error)
        }
    }

    /// Saves locally, then syncs in the background. Returns whether it saved.
    @discardableResult
    public func save() async -> Bool {
        guard canSave else { return false }
        busy = true
        defer { busy = false }
        do {
            let item = try assembled(checkingCode: true)
            _ = try await vault.account.save(item, in: vaultId)
            error = nil
            await vault.saved()
            return true
        } catch {
            self.error = message(for: error)
            return false
        }
    }

    /// The item with the edits applied. Empty URLs, lines and unnamed empty
    /// fields are dropped; the code is checked by the core when saving.
    func assembled(checkingCode: Bool) throws -> Item {
        var item = original
        let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        item.title = trimmedTitle
        item.notes = notes
        item.favorite = favorite
        item.folderId = folderId
        item.tags = tags.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        item.customFields = customFields.map(\.value).filter { !($0.name.isEmpty && $0.value.isEmpty) }
        switch original.kind {
        case .login:
            var login = self.login
            login.urls = urls.map(\.value).filter { !$0.url.trimmingCharacters(in: .whitespaces).isEmpty }
            let code = totp.trimmingCharacters(in: .whitespacesAndNewlines)
            if code.isEmpty {
                login.totp = nil
            } else if code != original.login?.totp {
                login.totp = checkingCode ? try OneTimeCode.normalize(code, title: trimmedTitle) : code
            }
            item.login = login
        case .card:
            var card = self.card
            card.expiryMonth = try number(expiryMonth, in: 1...12, field: "expiry_month")
            card.expiryYear = try number(expiryYear, in: 2000...2099, field: "expiry_year")
            item.card = card
        case .identity:
            var identity = self.identity
            identity.addressLines = address.split(separator: "\n", omittingEmptySubsequences: true)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }
            item.identity = identity
        case .note, .folder, nil:
            break
        }
        return item.sameFields(as: original) ? original : item
    }

    private func number(_ text: String, in range: ClosedRange<Int>, field: String) throws -> Int? {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return nil }
        guard let value = Int(trimmed), range.contains(value) else { throw PWMError(code: "invalid_\(field)") }
        return value
    }
}

extension Item {
    /// Equal once fields that were absent and are now empty are ignored, so
    /// opening and leaving an item unchanged is not an edit.
    func sameFields(as other: Item) -> Bool {
        Item.normalized(json) == Item.normalized(other.json)
    }

    private static func normalized(_ object: [String: JSONValue]) -> [String: JSONValue] {
        object.compactMapValues { value in
            switch value {
            case .null: nil
            case .string(""): nil
            case .bool(false): nil
            case let .array(items) where items.isEmpty: nil
            case let .object(inner):
                .object(normalized(inner))
            case let .array(items):
                .array(items.map { item in item.object.map { .object(normalized($0)) } ?? item })
            default: value
            }
        }
    }
}
