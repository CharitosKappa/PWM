// Editing an item, the generator and settings (D43 §2). Leaving an edit
// with changes asks first; the generator's values are never stored.
#if canImport(SwiftUI)
import PWMDesign
import PWMKit
import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

// MARK: Edit

struct ItemEditView: View {
    @Bindable var model: ItemEditModel
    @Environment(\.dismiss) private var dismiss
    @State private var confirmDiscard = false
    @State private var generator: GeneratorModel?
    @State private var scanning = false
    @State private var addingFolder = false
    @State private var folderName = ""

    var body: some View {
        Form {
            Section {
                TextField("Title", text: $model.title)
                    .accessibilityIdentifier("title")
                Toggle("Favorite", isOn: $model.favorite)
            }
            switch model.kind {
            case .login: loginFields
            case .card: cardFields
            case .identity: identityFields
            case .note, .folder, nil: EmptyView()
            }
            Section("Notes") {
                TextField("Notes", text: $model.notes, axis: .vertical)
                    .lineLimit(3...12)
            }
            customFields
            Section {
                Picker("Folder", selection: $model.folderId) {
                    Text("None").tag(String?.none)
                    ForEach(model.folders) { folder in
                        Text(verbatim: folder.item.title).tag(Optional(folder.id))
                    }
                }
                Button("New folder…") { addingFolder = true }
                TextField("Tags, separated by commas", text: $model.tags)
                    .plainInput()
            }
            ErrorText(text: model.error)
        }
        .navigationTitle(model.isNew ? "New item" : "Edit item")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") {
                    if model.hasChanges {
                        confirmDiscard = true
                    } else {
                        dismiss()
                    }
                }
                .accessibilityIdentifier("cancel")
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") {
                    Task { if await model.save() { dismiss() } }
                }
                .disabled(!model.canSave)
                .accessibilityIdentifier("save")
            }
        }
        .interactiveDismissDisabled(model.hasChanges)
        .confirmationDialog("Discard your changes?", isPresented: $confirmDiscard, titleVisibility: .visible) {
            Button("Discard changes", role: .destructive) { dismiss() }
            Button("Keep editing", role: .cancel) {}
        }
        .alert("New folder", isPresented: $addingFolder) {
            TextField("Name", text: $folderName)
            Button("Create") {
                let name = folderName
                folderName = ""
                Task { await model.newFolder(named: name) }
            }
            Button("Cancel", role: .cancel) { folderName = "" }
        }
        .sheet(item: $generator) { generator in
            NavigationStack {
                GeneratorView(model: generator) { value in
                    model.use(value)
                    self.generator = nil
                }
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { self.generator = nil }
                    }
                }
            }
        }
        .sheet(isPresented: $scanning) {
            QRScanner { text in
                model.totp = text
                scanning = false
            }
            .ignoresSafeArea()
        }
    }

    @ViewBuilder private var loginFields: some View {
        Section {
            TextField("Username", text: $model.login.username)
                .plainInput()
                .accessibilityIdentifier("username")
            HStack {
                RevealField(title: "Password", text: $model.login.password, identifier: "password")
                Button {
                    generator = model.makeGenerator()
                } label: {
                    Image(systemName: "wand.and.stars")
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Generate a password")
                .accessibilityIdentifier("generate")
            }
            HStack {
                TextField("One-time code setup key", text: $model.totp)
                    .plainInput()
                    .accessibilityIdentifier("totp")
                if QRScanner.isAvailable {
                    Button {
                        scanning = true
                    } label: {
                        Image(systemName: "qrcode.viewfinder")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Scan QR code")
                }
            }
        }
        Section("Websites") {
            ForEach($model.urls) { $row in
                HStack {
                    TextField("https://example.com", text: $row.value.url)
                        .plainInput()
                        .field(.url)
                        .accessibilityIdentifier("url")
                    MatchPicker(match: $row.value.match)
                }
            }
            .onDelete { model.urls.remove(atOffsets: $0) }
            Button("Add website") { model.addURL() }
                .accessibilityIdentifier("addURL")
        }
    }

    @ViewBuilder private var cardFields: some View {
        Section {
            TextField("Name on card", text: $model.card.cardholder)
            RevealField(title: "Number", text: $model.card.number, identifier: "number")
                .numericInput()
            HStack {
                TextField("Month (MM)", text: $model.expiryMonth)
                    .numericInput()
                TextField("Year (YYYY)", text: $model.expiryYear)
                    .numericInput()
            }
            RevealField(title: "Security code", text: $model.card.securityCode, identifier: "securityCode")
                .numericInput()
            TextField("Brand", text: $model.card.brand)
        }
    }

    @ViewBuilder private var identityFields: some View {
        Section {
            TextField("First name", text: $model.identity.firstName)
            TextField("Middle name", text: $model.identity.middleName)
            TextField("Last name", text: $model.identity.lastName)
            TextField("Email", text: $model.identity.email)
                .plainInput()
                .field(.email)
            TextField("Phone", text: $model.identity.phone)
            TextField("Company", text: $model.identity.company)
        }
        Section("Address") {
            TextField("Street address", text: $model.address, axis: .vertical)
                .lineLimit(1...4)
            TextField("City", text: $model.identity.city)
            TextField("Region", text: $model.identity.region)
            TextField("Postal code", text: $model.identity.postalCode)
            TextField("Country", text: $model.identity.country)
        }
    }

    @ViewBuilder private var customFields: some View {
        Section("Other fields") {
            ForEach($model.customFields) { $row in
                VStack(alignment: .leading) {
                    TextField("Name", text: $row.value.name)
                        .textStyle(Tokens.Text.caption)
                    switch row.value.kind {
                    case .hidden:
                        RevealField(title: "Value", text: $row.value.value, identifier: "fieldValue")
                    case .url:
                        TextField("Value", text: $row.value.value).plainInput().field(.url)
                    case .text, .date:
                        TextField("Value", text: $row.value.value)
                    case nil:
                        Text("Kept as it is: made by a newer version of the app.")
                            .textStyle(Tokens.Text.caption)
                            .foregroundStyle(Tokens.Color.textMuted.color)
                    }
                }
            }
            .onDelete { model.customFields.remove(atOffsets: $0) }
            Menu("Add field") {
                Button("Text") { model.addField(.text) }
                Button("Hidden") { model.addField(.hidden) }
                Button("Website") { model.addField(.url) }
                Button("Date") { model.addField(.date) }
            }
        }
    }
}

extension GeneratorModel: Identifiable {}

/// A secure field with Show, for editing secrets.
struct RevealField: View {
    let title: LocalizedStringKey
    @Binding var text: String
    let identifier: String
    @State private var shown = false

    var body: some View {
        HStack {
            Group {
                if shown {
                    TextField(title, text: $text)
                        .plainInput()
                        .font(.system(.body, design: .monospaced))
                } else {
                    SecureField(title, text: $text)
                }
            }
            .accessibilityIdentifier(identifier)
            Button {
                shown.toggle()
            } label: {
                Image(systemName: shown ? "eye.slash" : "eye")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(shown ? "Hide" : "Show")
        }
    }
}

/// How a website matches pages for autofill (`AUTOFILL.md`). A mode from
/// a newer app version is shown as it is and kept.
struct MatchPicker: View {
    @Binding var match: String

    private static let modes: [(String, LocalizedStringKey)] = [
        ("domain", "Domain"), ("host", "Host"), ("starts_with", "Starts with"),
        ("exact", "Exact"), ("regex", "Regex"), ("never", "Never"),
    ]

    var body: some View {
        Picker("Match", selection: $match) {
            ForEach(Self.modes, id: \.0) { mode in
                Text(mode.1).tag(mode.0)
            }
            if !Self.modes.contains(where: { $0.0 == match }) {
                Text(verbatim: match).tag(match)
            }
        }
        .labelsHidden()
        .fixedSize()
    }
}

extension View {
    /// Digits only, for card fields.
    func numericInput() -> some View {
        #if os(iOS)
        self.keyboardType(.numberPad)
        #else
        self
        #endif
    }
}

// MARK: Generator

struct GeneratorView: View {
    @Bindable var model: GeneratorModel
    /// Set when the generator fills a password field.
    let onUse: ((Redacted<String>) -> Void)?

    var body: some View {
        Form {
            Section {
                Text(verbatim: model.hiddenByCapture ? ScreenCapture.hiddenLine : model.value?.value ?? " ")
                    .font(.system(.title3, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
                    .accessibilityIdentifier("generated")
                StrengthMeter(strength: model.strength)
                HStack {
                    Button {
                        Task { await model.regenerate() }
                    } label: {
                        Label("New", systemImage: "arrow.clockwise")
                    }
                    Spacer()
                    Button(model.copied ? "Copied" : "Copy") { model.copy() }
                        .disabled(model.value == nil)
                        .accessibilityIdentifier("copyGenerated")
                }
                .buttonStyle(.borderless)
                if let onUse {
                    Button("Use this password") {
                        if let value = model.value { onUse(value) }
                    }
                    .disabled(model.value == nil)
                    .accessibilityIdentifier("useGenerated")
                }
            } footer: {
                Text(model.copied ? "The clipboard clears in 30 seconds." : " ")
            }
            Section {
                Picker("Type", selection: $model.mode) {
                    Text("Password").tag(GeneratorModel.Mode.password)
                    Text("Passphrase").tag(GeneratorModel.Mode.passphrase)
                }
                .pickerStyle(.segmented)
                switch model.mode {
                case .password:
                    Stepper(value: $model.length, in: GeneratorModel.lengths) {
                        Text("Length: \(model.length)")
                    }
                    Toggle("Lowercase (a–z)", isOn: $model.lowercase)
                    Toggle("Uppercase (A–Z)", isOn: $model.uppercase)
                    Toggle("Digits (0–9)", isOn: $model.digits)
                    Toggle("Symbols (!#$%…)", isOn: $model.symbols)
                    Toggle("Avoid look-alikes (0, O, 1, l)", isOn: $model.avoidLookalikes)
                case .passphrase:
                    Stepper(value: $model.words, in: GeneratorModel.wordCounts) {
                        Text("Words: \(model.words)")
                    }
                    Picker("Separator", selection: $model.separator) {
                        ForEach(GeneratorModel.separators, id: \.self) { separator in
                            Text(verbatim: separator == " " ? "Space" : separator).tag(separator)
                        }
                    }
                    Toggle("Capitalize", isOn: $model.capitalize)
                    Toggle("Add a digit", isOn: $model.addDigit)
                }
            }
            ErrorText(text: model.error)
        }
        .navigationTitle("Generator")
        .task(id: model.options) { await model.regenerate() }
    }
}

// MARK: Settings

extension LockTimeout {
    var label: LocalizedStringKey {
        switch self {
        case .immediately: "Immediately"
        case .oneMinute: "After 1 minute"
        case .fiveMinutes: "After 5 minutes"
        case .fifteenMinutes: "After 15 minutes"
        case .oneHour: "After 1 hour"
        case .eightHours: "After 8 hours"
        }
    }
}

struct SettingsView: View {
    let model: SettingsModel
    @State private var confirmSignOut = false
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.openURL) private var openURL

    private var lockTimeout: Binding<LockTimeout> {
        Binding(get: { model.lockTimeout }, set: { model.setLockTimeout($0) })
    }

    private var breachCheck: Binding<Bool> {
        Binding(get: { model.breachCheckOn }, set: { model.setBreachCheck($0) })
    }

    private var suggestLogins: Binding<Bool> {
        Binding(get: { model.suggestLogins }, set: { on in Task { await model.setSuggestLogins(on) } })
    }

    var body: some View {
        Form {
            Section("Account") {
                LabeledContent("Email") { Text(verbatim: model.email ?? "") }
                LabeledContent("Server") {
                    if let server = model.server {
                        Text(verbatim: server.absoluteString)
                    } else {
                        Text("Hosted by us")
                    }
                }
            }
            if model.autofillOn == false {
                Section {
                    #if os(macOS)
                    Text("Turn on PWM in System Settings, under AutoFill & Passwords, to fill passwords and passkeys in Safari and other apps.")
                    #else
                    Text("Turn on PWM in Settings, under General, AutoFill & Passwords, to fill passwords and passkeys in Safari and other apps.")
                    #endif
                    Button("Open AutoFill settings") { Task { await model.openAutoFillSettings() } }
                        .accessibilityIdentifier("openAutoFill")
                } header: {
                    Text("AutoFill")
                }
            }
            BiometricSetting(model: model)
            Section {
                Toggle("Suggest logins above the keyboard", isOn: suggestLogins)
                    .accessibilityIdentifier("suggestLogins")
            } footer: {
                Text("The system keeps the site and username of each login on this device to suggest them. Passwords stay in your vault.")
            }
            Section {
                NavigationLink("Password health") { HealthHost(settings: model) }
                    .accessibilityIdentifier("health")
                Toggle("Check for leaked passwords", isOn: breachCheck)
                    .accessibilityIdentifier("breachCheck")
            } footer: {
                Text("Checks your passwords against known leaks without sending them: only the first 5 characters of a hash leave your device.")
            }
            #if os(macOS)
            if let on = model.browserTouchIDOn {
                Section {
                    Toggle("Allow browser extensions to unlock with Touch ID", isOn: Binding(
                        get: { on }, set: { model.setBrowserTouchID($0) }
                    ))
                    .accessibilityIdentifier("browserTouchID")
                } footer: {
                    Text("For PWM in Chrome, Edge, Brave, Opera and Firefox. Turn it on there too. Your master password is still asked every 14 days.")
                }
            }
            Section {
                Toggle("Hide PWM from screenshots and screen sharing", isOn: Binding(
                    get: { model.hidesFromCaptureOn }, set: { model.setHidesFromCapture($0) }
                ))
                .accessibilityIdentifier("hideFromCapture")
            } footer: {
                Text("While unlocked, the PWM window is left out of screenshots, recordings and screen sharing. Turn this off to show it when you share your screen.")
            }
            #endif
            Section {
                NavigationLink("Devices") { DevicesHost(settings: model) }
                    .accessibilityIdentifier("devices")
                NavigationLink {
                    ApprovalsHost(settings: model)
                } label: {
                    LabeledContent("Approve a new device") {
                        if model.pendingApprovals > 0 {
                            Text(verbatim: String(model.pendingApprovals))
                        }
                    }
                }
                .accessibilityIdentifier("approvals")
                Toggle("Notify me when a new device asks to sign in", isOn: Binding(
                    get: { model.approvalNoticesOn },
                    set: { on in
                        Task {
                            if let url = await model.setApprovalNotices(on) { openURL(url) }
                        }
                    }
                ))
                .accessibilityIdentifier("approvalNotices")
                NavigationLink("Import") { ImportHost(settings: model) }
                    .accessibilityIdentifier("importVault")
                NavigationLink("Export vault") { ExportHost(settings: model) }
                    .accessibilityIdentifier("exportVault")
            } footer: {
                Text("Export works on every plan.")
            }
            Section {
                Picker("Lock the vault", selection: lockTimeout) {
                    ForEach(LockTimeout.allCases, id: \.self) { timeout in
                        Text(timeout.label).tag(timeout)
                    }
                }
                .accessibilityIdentifier("lockTimeout")
                Button("Lock now") { Task { await model.lockNow() } }
                    .accessibilityIdentifier("lockNow")
            } footer: {
                Text("Counted while the app is in the background. Locking your device always locks the vault.")
            }
            Section {
                if let kit = model.kit {
                    ShareLink(item: KitFile(data: kit.value), preview: SharePreview("Emergency Kit")) {
                        Label("Save Emergency Kit", systemImage: "square.and.arrow.down")
                    }
                } else {
                    ProgressView()
                }
            } footer: {
                Text("Your Secret Key and email, for signing in on a new device. Print it or keep it offline.")
            }
            Section {
                NavigationLink("Third-party notices") { NoticesView() }
                LabeledContent("Version") { Text(verbatim: model.version) }
            }
            Section {
                Button("Sign out", role: .destructive) { confirmSignOut = true }
                    .accessibilityIdentifier("signOut")
            }
            ErrorText(text: model.error)
        }
        .navigationTitle("Settings")
        .task {
            await model.load()
            await model.prepareKit()
        }
        .onDisappear { model.dropKit() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await model.loadAutoFill() } }
        }
        .task(id: model.signOutAsked) {
            guard model.signOutAsked else { return }
            confirmSignOut = true
            model.signOutQuestionShown()
        }
        .confirmationDialog("Sign out of this device?", isPresented: $confirmSignOut, titleVisibility: .visible) {
            Button("Sign out", role: .destructive) { Task { await model.signOut() } }
                .accessibilityIdentifier("confirmSignOut")
        } message: {
            Text("Your vault is removed from this device. Changes that haven't synced yet are lost.")
        }
    }
}

/// "Unlock with Face ID" (D45 §3): asks for Face ID once to turn on.
struct BiometricSetting: View {
    let model: SettingsModel

    var body: some View {
        switch model.biometric {
        case let .off(kind), let .on(kind):
            Section {
                Toggle(isOn: Binding(
                    get: { if case .on = model.biometric { true } else { false } },
                    set: { on in Task { await model.setBiometric(on) } }
                )) {
                    Text("Unlock with \(Text(kind.label))")
                }
                .accessibilityIdentifier("biometricSwitch")
            } footer: {
                Text("Your master password is still asked every 14 days, and after Face ID or Touch ID changes.")
            }
        case let .unavailable(reason):
            Section {
                Text(verbatim: reason)
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
                    .accessibilityIdentifier("biometricUnavailable")
            } header: {
                Text("Face ID and Touch ID")
            }
        case nil:
            EmptyView()
        }
    }
}

/// The licences of the core's dependencies and data, bundled by
/// `just notices` (S6). Builds without them say so.
struct NoticesView: View {
    @State private var paragraphs: [String]?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: Tokens.Space.s3) {
                if let paragraphs, !paragraphs.isEmpty {
                    ForEach(Array(paragraphs.enumerated()), id: \.offset) { _, text in
                        Text(verbatim: text).textStyle(Tokens.Text.caption)
                    }
                } else if paragraphs != nil {
                    Text("This build has no notices. They are added to release builds.")
                }
            }
            .padding(Tokens.Space.s4)
            .textSelection(.enabled)
        }
        .navigationTitle("Third-party notices")
        .task { paragraphs = Notices.load() }
    }
}

enum Notices {
    /// The bundled notices as plain paragraphs.
    @MainActor static func load() -> [String] {
        var text = ""
        if let url = Bundle.main.url(forResource: "rust", withExtension: "html"),
           let data = try? Data(contentsOf: url),
           let html = try? NSAttributedString(
               data: data,
               options: [
                   .documentType: NSAttributedString.DocumentType.html,
                   .characterEncoding: String.Encoding.utf8.rawValue,
               ],
               documentAttributes: nil
           )
        {
            text += html.string
        }
        for name in ["data", "data-autofill"] {
            if let url = Bundle.main.url(forResource: name, withExtension: "md"),
               let more = try? String(contentsOf: url, encoding: .utf8)
            {
                text += "\n\n" + more
            }
        }
        return text.components(separatedBy: "\n\n")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }
}
#endif
