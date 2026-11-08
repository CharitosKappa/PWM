// The vault's screens (D43 §2): three tabs on iPhone, and the vault as a
// split view with its sections as a sidebar on iPad (and macOS with M5.6).
// No website icons: fetching them would tell others which sites are saved.
#if canImport(SwiftUI)
import PWMDesign
import PWMKit
import SwiftUI

/// Vault, Generator and Settings.
struct MainView: View {
    let app: AppModel

    var body: some View {
        if let vault = app.vault, let generator = app.generator, let settings = app.settings {
            #if os(macOS)
            MacMainView(app: app, vault: vault, generator: generator, settings: settings)
            #else
            TabView {
                VaultView(model: vault)
                    .tabItem { Label("Vault", systemImage: "lock.rectangle.stack") }
                NavigationStack { GeneratorView(model: generator, onUse: nil) }
                    .tabItem { Label("Generator", systemImage: "dice") }
                NavigationStack { SettingsView(model: settings) }
                    .tabItem { Label("Settings", systemImage: "gearshape") }
                    .badge(settings.pendingApprovals)
            }
            #endif
        }
    }
}

extension Item.Kind {
    var label: LocalizedStringKey {
        switch self {
        case .login: "Login"
        case .card: "Card"
        case .identity: "Identity"
        case .note: "Note"
        case .folder: "Folder"
        }
    }

    var sectionLabel: LocalizedStringKey {
        switch self {
        case .login: "Logins"
        case .card: "Cards"
        case .identity: "Identities"
        case .note: "Notes"
        case .folder: "Folders"
        }
    }

    var symbol: String {
        switch self {
        case .login: "key"
        case .card: "creditcard"
        case .identity: "person.text.rectangle"
        case .note: "note.text"
        case .folder: "folder"
        }
    }
}

extension ItemEditModel: Identifiable {}

/// The smallest touch target (design/README.md): 44 pt on iOS, 24 on the Mac.
#if os(macOS)
let minimumTarget: CGFloat = 24
#else
let minimumTarget: CGFloat = 44
#endif

/// A section title VoiceOver can jump to.
struct SectionHeader: View {
    let title: LocalizedStringKey

    init(_ title: LocalizedStringKey) {
        self.title = title
    }

    var body: some View {
        Text(title).accessibilityAddTraits(.isHeader)
    }
}

// MARK: The vault

struct VaultView: View {
    @Bindable var model: VaultModel
    @State private var column = NavigationSplitViewColumn.content
    @State private var selection: String?
    @State private var editing: ItemEditModel?

    var body: some View {
        NavigationSplitView(preferredCompactColumn: $column) {
            SectionList(model: model)
        } content: {
            ItemList(model: model, selection: $selection, editing: $editing)
        } detail: {
            if let entry = model.entry(selection) {
                DetailHost(entry: entry, vault: model) {
                    editing = model.edit(entry)
                }
                .id(entry.id + String(entry.item.updatedAtMs ?? 0) + String(entry.item.trashedAtMs ?? 0))
            } else {
                Text("Select an item")
                    .foregroundStyle(Tokens.Color.textMuted.color)
            }
        }
        .task {
            await model.load()
            await model.refresh()
        }
        .sheet(item: $editing) { edit in
            NavigationStack { ItemEditView(model: edit) }
        }
    }
}

struct SectionList: View {
    @Bindable var model: VaultModel

    private var selection: Binding<VaultModel.Section?> {
        Binding(get: { model.section }, set: { if let section = $0 { model.section = section } })
    }

    var body: some View {
        List(selection: selection) {
            Label("All items", systemImage: "tray.full").tag(VaultModel.Section.all)
            Label("Favorites", systemImage: "star").tag(VaultModel.Section.favorites)
            ForEach(VaultModel.kinds, id: \.self) { kind in
                Label(kind.sectionLabel, systemImage: kind.symbol).tag(VaultModel.Section.kind(kind))
            }
            if !model.folders.isEmpty {
                Section {
                    ForEach(model.folders) { folder in
                        Label(folder.item.title, systemImage: "folder").tag(VaultModel.Section.folder(folder.id))
                    }
                } header: {
                    SectionHeader("Folders")
                }
            }
            Label("Trash", systemImage: "trash").tag(VaultModel.Section.trash)
        }
        .navigationTitle("Vault")
    }
}

struct ItemList: View {
    @Bindable var model: VaultModel
    @Binding var selection: String?
    @Binding var editing: ItemEditModel?
    @State private var deleting: Entry?
    @State private var signingInAgain = false

    private var title: String {
        switch model.section {
        case .all: String(localized: "All items")
        case .favorites: String(localized: "Favorites")
        case .trash: String(localized: "Trash")
        case let .folder(id): model.entry(id)?.item.title ?? ""
        case let .kind(kind):
            switch kind {
            case .login: String(localized: "Logins")
            case .card: String(localized: "Cards")
            case .identity: String(localized: "Identities")
            case .note: String(localized: "Notes")
            case .folder: String(localized: "Folders")
            }
        }
    }

    var body: some View {
        List(model.visible, selection: $selection) { entry in
            ItemRow(entry: entry) { field in
                Task { await model.copy(field, of: entry) }
            }
                .swipeActions {
                    if entry.item.trashedAtMs == nil {
                        Button("Delete", role: .destructive) { Task { await model.trash(entry) } }
                    } else {
                        Button("Delete forever", role: .destructive) { deleting = entry }
                        Button("Restore") { Task { await model.restore(entry) } }
                    }
                }
        }
        .navigationTitle(title)
        .searchable(text: $model.query)
        // ⌘N on macOS (D48 §2).
        .task(id: model.pendingNew) {
            guard let kind = model.pendingNew else { return }
            editing = await model.newItem(kind)
            model.pendingNew = nil
        }
        .refreshable { await model.refresh() }
        .overlay {
            if model.loaded && model.visible.isEmpty {
                if model.query.isEmpty {
                    ContentUnavailableView(
                        model.section == .trash ? LocalizedStringKey("The trash is empty") : LocalizedStringKey("No items"),
                        systemImage: "tray"
                    )
                } else {
                    ContentUnavailableView.search(text: model.query)
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            if let again = model.signInAgain, again.needed {
                Button("Sign in again to sync") { signingInAgain = true }
                    .textStyle(Tokens.Text.caption)
                    .padding(Tokens.Space.s2)
                    .frame(maxWidth: .infinity)
                    .background(.bar)
                    .accessibilityIdentifier("signInAgain")
            } else if let notice = model.error ?? model.status {
                Text(verbatim: notice)
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
                    .padding(Tokens.Space.s2)
                    .frame(maxWidth: .infinity)
                    .background(.bar)
                    .accessibilityIdentifier("status")
            }
        }
        .toolbar {
            if model.section != .trash {
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        ForEach(VaultModel.kinds, id: \.self) { kind in
                            Button {
                                Task { editing = await model.newItem(kind) }
                            } label: {
                                Label(kind.label, systemImage: kind.symbol)
                            }
                            .accessibilityIdentifier("new.\(kind.rawValue)")
                        }
                    } label: {
                        Label("New item", systemImage: "plus")
                    }
                    .accessibilityIdentifier("newItem")
                }
            }
        }
        .sheet(isPresented: $signingInAgain) {
            if let again = model.signInAgain {
                SignInAgainView(model: again) { signingInAgain = false }
            }
        }
        .confirmationDialog(
            "Delete this item forever?",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible,
            presenting: deleting
        ) { entry in
            Button("Delete forever", role: .destructive) { Task { await model.deleteForever(entry) } }
        } message: { _ in
            Text("It is removed from all your devices and can't be restored.")
        }
    }
}

/// "Sign in again to sync" (D50 §5): the master password only. What this
/// device hasn't sent yet is kept.
struct SignInAgainView: View {
    @Bindable var model: SignInAgainModel
    let done: () -> Void

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    SecureField("Master password", text: $model.password)
                        .field(.password)
                        .accessibilityIdentifier("signInAgainPassword")
                        .onSubmit { signIn() }
                } footer: {
                    Text("Your session on this device ended. Changes you made here are kept and sync once you sign in.")
                }
                ErrorText(text: model.error)
            }
            .navigationTitle("Sign in again")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { done() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Sign in") { signIn() }
                        .disabled(!model.canSignIn)
                        .accessibilityIdentifier("signInAgainButton")
                }
            }
        }
    }

    private func signIn() {
        Task {
            if await model.signIn() { done() }
        }
    }
}

/// One VoiceOver stop: name, username and "has a one-time code", with the
/// copies as its actions.
struct ItemRow: View {
    let entry: Entry
    var copy: ((VaultModel.RowCopy) -> Void)?

    private var subtitle: String {
        if let login = entry.item.login { return login.username }
        if let card = entry.item.card, !card.lastFour.isEmpty { return "•••• \(card.lastFour)" }
        if let identity = entry.item.identity { return [identity.firstName, identity.lastName].filter { !$0.isEmpty }.joined(separator: " ") }
        return ""
    }

    var body: some View {
        HStack(spacing: Tokens.Space.s3) {
            Image(systemName: entry.item.kind?.symbol ?? "questionmark.square")
                .frame(width: 28)
                .foregroundStyle(Tokens.Color.accent.color)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: entry.item.title.isEmpty ? " " : entry.item.title)
                    .textStyle(Tokens.Text.bodyStrong)
                    .lineLimit(1)
                if !subtitle.isEmpty {
                    Text(verbatim: subtitle)
                        .textStyle(Tokens.Text.caption)
                        .foregroundStyle(Tokens.Color.textMuted.color)
                        .lineLimit(1)
                }
            }
            Spacer()
            if entry.item.favorite {
                Image(systemName: "star.fill")
                    .foregroundStyle(Tokens.Color.warning.color)
                    .accessibilityLabel("Favorite")
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: entry.spokenLabel))
        .accessibilityIdentifier("item")
        .accessibilityActions {
            if let copy, let login = entry.item.login, entry.item.trashedAtMs == nil {
                if !login.password.isEmpty {
                    Button("Copy password") { copy(.password) }
                }
                if !login.username.isEmpty {
                    Button("Copy username") { copy(.username) }
                }
                if let totp = login.totp, !totp.isEmpty {
                    Button("Copy code") { copy(.code) }
                }
            }
        }
    }
}

// MARK: Detail

/// Keeps one detail model per shown item, so revealed fields stay revealed
/// while the vault redraws.
struct DetailHost: View {
    let vault: VaultModel
    let onEdit: () -> Void
    @State private var model: ItemDetailModel

    init(entry: Entry, vault: VaultModel, onEdit: @escaping () -> Void) {
        self.vault = vault
        self.onEdit = onEdit
        _model = State(initialValue: vault.detail(entry))
    }

    var body: some View {
        ItemDetailView(model: model, vault: vault, onEdit: onEdit)
    }
}

struct ItemDetailView: View {
    let model: ItemDetailModel
    let vault: VaultModel
    let onEdit: () -> Void
    @State private var deleting = false

    private var item: Item { model.item }

    var body: some View {
        List {
            if !item.isEditable {
                Section {
                    Text("This item was made by a newer version of the app. Update the app to edit it.")
                        .textStyle(Tokens.Text.caption)
                }
            }
            if let notice = model.notice {
                Section {
                    Text(verbatim: notice)
                        .textStyle(Tokens.Text.caption)
                        .foregroundStyle(Tokens.Color.textMuted.color)
                        .accessibilityIdentifier("captureNotice")
                }
            }
            switch item.kind {
            case .login: loginFields
            case .card: cardFields
            case .identity: identityFields
            case .note, .folder, nil: EmptyView()
            }
            if !item.notes.isEmpty {
                Section {
                    Text(verbatim: item.notes).textSelection(.enabled)
                } header: {
                    SectionHeader("Notes")
                }
            }
            if !item.customFields.isEmpty {
                Section {
                    ForEach(Array(item.customFields.enumerated()), id: \.offset) { index, field in
                        FieldRow(
                            label: field.name, value: field.value, key: "field.\(index)",
                            hidden: field.kind == .hidden || field.kind == nil, model: model
                        )
                    }
                } header: {
                    SectionHeader("Other fields")
                }
            }
            Section {
                if let folder = vault.entry(item.folderId) {
                    LabeledContent("Folder") { Text(verbatim: folder.item.title) }
                }
                if !item.tags.isEmpty {
                    LabeledContent("Tags") { Text(verbatim: item.tags.joined(separator: ", ")) }
                }
                if let updated = item.updatedAtMs {
                    LabeledContent("Updated") { Text(Self.date(updated), style: .date) }
                }
            }
            Section {
                if item.trashedAtMs == nil {
                    Button("Delete", role: .destructive) { Task { await vault.trash(model.entry) } }
                        .accessibilityIdentifier("trash")
                } else {
                    Button("Restore") { Task { await vault.restore(model.entry) } }
                        .accessibilityIdentifier("restore")
                    Button("Delete forever", role: .destructive) { deleting = true }
                }
            }
        }
        .navigationTitle(Text(verbatim: item.title))
        .toolbar {
            if item.trashedAtMs == nil {
                ToolbarItem(placement: .primaryAction) {
                    Button("Edit", action: onEdit)
                        .disabled(!item.isEditable)
                        .accessibilityIdentifier("edit")
                }
            }
        }
        .onDisappear { model.hideAll() }
        .confirmationDialog("Delete this item forever?", isPresented: $deleting, titleVisibility: .visible) {
            Button("Delete forever", role: .destructive) { Task { await vault.deleteForever(model.entry) } }
        } message: {
            Text("It is removed from all your devices and can't be restored.")
        }
    }

    /// Only web addresses open in the browser.
    static func webLink(_ text: String) -> URL? {
        guard let link = URL(string: text), ["https", "http"].contains(link.scheme?.lowercased()) else { return nil }
        return link
    }

    static func date(_ ms: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
    }

    @ViewBuilder private var loginFields: some View {
        if let login = item.login {
            Section {
                if !login.username.isEmpty {
                    FieldRow(label: String(localized: "Username"), value: login.username, key: "username", model: model)
                }
                if !login.password.isEmpty {
                    FieldRow(label: String(localized: "Password"), value: login.password, key: "password", hidden: true, monospaced: true, model: model)
                }
                if let totp = login.totp, !totp.isEmpty {
                    CodeRow(model: model)
                        .task(id: totp) {
                            while !Task.isCancelled {
                                await model.refreshCode()
                                try? await Task.sleep(for: .seconds(1))
                            }
                        }
                }
            }
            if !login.urls.isEmpty {
                Section {
                    ForEach(Array(login.urls.enumerated()), id: \.offset) { index, url in
                        FieldRow(
                            label: String(localized: "Website"), value: url.url, key: "url.\(index)",
                            link: Self.webLink(url.url), model: model
                        )
                    }
                } header: {
                    SectionHeader("Websites")
                }
            }
            if !login.passkeySites.isEmpty {
                Section {
                    ForEach(Array(login.passkeySites.enumerated()), id: \.offset) { _, site in
                        Label { Text("Passkey for \(site)") } icon: { Image(systemName: "person.badge.key") }
                    }
                } header: {
                    SectionHeader("Passkeys")
                }
            }
            if !item.passwordHistory.isEmpty {
                Section {
                    ForEach(Array(item.passwordHistory.enumerated()), id: \.offset) { index, previous in
                        FieldRow(
                            label: previous.replacedAtMs.map { Self.date($0).formatted(date: .abbreviated, time: .shortened) } ?? "",
                            value: previous.password, key: "history.\(index)", hidden: true, monospaced: true, model: model
                        )
                    }
                } header: {
                    SectionHeader("Previous passwords")
                }
            }
        }
    }

    @ViewBuilder private var cardFields: some View {
        if let card = item.card {
            Section {
                if !card.cardholder.isEmpty {
                    FieldRow(label: String(localized: "Name on card"), value: card.cardholder, key: "cardholder", model: model)
                }
                if !card.number.isEmpty {
                    FieldRow(label: String(localized: "Number"), value: card.number, key: "number", hidden: true, monospaced: true, model: model)
                }
                if let month = card.expiryMonth, let year = card.expiryYear {
                    FieldRow(label: String(localized: "Expires"), value: String(format: "%02d/%d", month, year), key: "expiry", model: model)
                }
                if !card.securityCode.isEmpty {
                    FieldRow(label: String(localized: "Security code"), value: card.securityCode, key: "securityCode", hidden: true, monospaced: true, model: model)
                }
                if !card.brand.isEmpty {
                    LabeledContent("Brand") { Text(verbatim: card.brand) }
                }
            }
        }
    }

    @ViewBuilder private var identityFields: some View {
        if let identity = item.identity {
            let name = [identity.firstName, identity.middleName, identity.lastName].filter { !$0.isEmpty }.joined(separator: " ")
            let address = (identity.addressLines + [identity.city, identity.region, identity.postalCode, identity.country])
                .filter { !$0.isEmpty }.joined(separator: "\n")
            Section {
                ForEach(
                    [
                        ("name", String(localized: "Name"), name),
                        ("email", String(localized: "Email"), identity.email),
                        ("phone", String(localized: "Phone"), identity.phone),
                        ("company", String(localized: "Company"), identity.company),
                        ("address", String(localized: "Address"), address),
                    ].filter { !$0.2.isEmpty },
                    id: \.0
                ) { key, label, value in
                    FieldRow(label: label, value: value, key: key, model: model)
                }
            }
        }
    }
}

/// A field with Copy, and Show for hidden values. One VoiceOver stop: its
/// label and value ("hidden" while hidden), with the buttons as its actions.
struct FieldRow: View {
    let label: String
    let value: String
    let key: String
    var hidden = false
    var monospaced = false
    /// A website's address, for Open website.
    var link: URL?
    let model: ItemDetailModel
    @Environment(\.openURL) var openURL

    var body: some View {
        let shown = !hidden || model.isRevealed(key)
        let captured = hidden && model.hiddenByCapture
        HStack(spacing: Tokens.Space.s2) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: label)
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
                Text(verbatim: captured ? ScreenCapture.hiddenLine : shown ? value : ItemDetailModel.masked(value))
                    .font(monospaced ? .secret(.body) : .app(.body))
                    .textSelection(.enabled)
            }
            Spacer()
            if hidden {
                Button {
                    model.toggle(key)
                } label: {
                    Image(systemName: shown ? "eye.slash" : "eye")
                        .frame(minWidth: minimumTarget, minHeight: minimumTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .disabled(captured)
                .accessibilityLabel(shown ? "Hide" : "Show")
            }
            Button {
                model.copy(value, field: key)
            } label: {
                Image(systemName: model.copied == key ? "checkmark" : "doc.on.doc")
                    .frame(minWidth: minimumTarget, minHeight: minimumTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(model.copied == key ? "Copied" : "Copy")
            if let link {
                Link(destination: link) {
                    Image(systemName: "arrow.up.right.square")
                        .frame(minWidth: minimumTarget, minHeight: minimumTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Open website")
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: label))
        .accessibilityValue(spokenValue(shown: shown, captured: captured))
        .accessibilityIdentifier("field.\(key)")
        .accessibilityActions {
            if hidden && !captured {
                Button(shown ? "Hide" : "Show") { model.toggle(key) }
            }
            Button("Copy") { model.copy(value, field: key) }
            if let link {
                Button("Open website") { openURL(link) }
            }
        }
    }

    /// Hidden values are never spoken, not even as dots; a shown secret is
    /// read one character at a time.
    private func spokenValue(shown: Bool, captured: Bool) -> Text {
        if captured { return Text(verbatim: ScreenCapture.hiddenLine) }
        if !shown { return Text(verbatim: ItemDetailModel.hiddenWord) }
        return hidden ? Text(verbatim: value).speechSpellsOutCharacters() : Text(verbatim: value)
    }
}

/// The one-time code and how long it stays valid. One VoiceOver stop, read
/// in digit groups; the countdown is extra content, so it isn't read out
/// every second.
struct CodeRow: View {
    let model: ItemDetailModel

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text("One-time code")
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
                if model.codeUnsupported {
                    Text("Unsupported code type").foregroundStyle(Tokens.Color.textMuted.color)
                } else if model.hiddenByCapture, model.code != nil {
                    Text(verbatim: ScreenCapture.hiddenLine)
                        .foregroundStyle(Tokens.Color.textMuted.color)
                } else if let code = model.code {
                    Text(verbatim: code.value.value)
                        .font(.secret(.title3))
                } else {
                    Text(verbatim: " ")
                }
            }
            Spacer()
            if let code = model.code {
                Text("\(code.secondsLeft) s")
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
                    .monospacedDigit()
                Button {
                    model.copyCode()
                } label: {
                    Image(systemName: model.copied == "code" ? "checkmark" : "doc.on.doc")
                        .frame(minWidth: minimumTarget, minHeight: minimumTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Copy one-time code")
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("One-time code")
        .accessibilityValue(spokenValue)
        .accessibilityIdentifier("code")
        .accessibilityCustomContent(AccessibilityCustomContentKey("Time left"), model.code.map { Text("\($0.secondsLeft) seconds") })
        .accessibilityActions {
            if model.code != nil {
                Button("Copy code") { model.copyCode() }
            }
        }
    }

    private var spokenValue: Text {
        if model.codeUnsupported { return Text("Unsupported code type") }
        guard let code = model.code else { return Text(verbatim: "") }
        if model.hiddenByCapture { return Text(verbatim: ScreenCapture.hiddenLine) }
        return Text(verbatim: ItemDetailModel.grouped(code.value.value)).speechSpellsOutCharacters()
    }
}
#endif
