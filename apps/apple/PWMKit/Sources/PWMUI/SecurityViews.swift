// The health report, devices and export screens (D43 §6–8), from Settings.
#if canImport(SwiftUI)
import PWMDesign
import PWMKit
import SwiftUI
import UniformTypeIdentifiers

// MARK: Hosts

/// Each screen keeps one model while it shows.
struct HealthHost: View {
    @State private var model: HealthModel?

    init(settings: SettingsModel) {
        _model = State(initialValue: settings.makeHealth())
    }

    var body: some View {
        if let model { HealthView(model: model) }
    }
}

struct DevicesHost: View {
    @State private var model: DevicesModel

    init(settings: SettingsModel) {
        _model = State(initialValue: settings.makeDevices())
    }

    var body: some View { DevicesView(model: model) }
}

struct ApprovalsHost: View {
    @State private var model: ApprovalsModel

    init(settings: SettingsModel) {
        _model = State(initialValue: settings.makeApprovals())
    }

    var body: some View { ApprovalsView(model: model) }
}

struct ExportHost: View {
    @State private var model: ExportModel

    init(settings: SettingsModel) {
        _model = State(initialValue: settings.makeExport())
    }

    var body: some View { ExportView(model: model) }
}

// MARK: Health

struct HealthView: View {
    let model: HealthModel
    @State private var editing: ItemEditModel?

    var body: some View {
        List {
            if model.checkingLeaks {
                Section {
                    HStack(spacing: Tokens.Space.s2) {
                        ProgressView()
                        Text("Checking for leaked passwords…")
                    }
                }
            }
            if !model.leaked.isEmpty {
                Section {
                    ForEach(model.leaked) { leaked in
                        HealthRow(entry: leaked.entry, detail: leakText(leaked.count)) { editing = model.edit(leaked.entry) }
                    }
                } header: {
                    SectionHeader("Leaked")
                } footer: {
                    Text("These passwords appear in known data breaches. Change them first.")
                }
            }
            if !model.weak.isEmpty {
                Section {
                    ForEach(model.weak) { entry in
                        HealthRow(entry: entry, detail: String(localized: "Easy to guess")) { editing = model.edit(entry) }
                    }
                } header: {
                    SectionHeader("Weak")
                }
            }
            if !model.reused.isEmpty {
                Section {
                    ForEach(Array(model.reused.enumerated()), id: \.offset) { _, group in
                        ForEach(group) { entry in
                            HealthRow(entry: entry, detail: String(localized: "Same password as \(group.count - 1) other")) {
                                editing = model.edit(entry)
                            }
                        }
                    }
                } header: {
                    SectionHeader("Reused")
                } footer: {
                    Text("A leak on one site would open the others. Give each its own password.")
                }
            }
            if model.isClean && !model.checkingLeaks {
                ContentUnavailableView("No problems found", systemImage: "checkmark.shield")
                    .accessibilityIdentifier("healthClean")
            }
            if let error = model.error {
                Section { ErrorText(text: error) }
            }
        }
        .navigationTitle("Password health")
        .task { await model.load() }
        .refreshable { await model.load() }
        .sheet(item: $editing, onDismiss: { Task { await model.load() } }) { edit in
            NavigationStack { ItemEditView(model: edit) }
        }
    }

    private func leakText(_ count: UInt64) -> String {
        String(localized: "Seen \(count.formatted()) times in leaks")
    }
}

struct HealthRow: View {
    let entry: Entry
    let detail: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: entry.item.title).textStyle(Tokens.Text.bodyStrong)
                    Text(verbatim: detail)
                        .textStyle(Tokens.Text.caption)
                        .foregroundStyle(Tokens.Color.textMuted.color)
                }
                Spacer()
                Text("Change").foregroundStyle(Tokens.Color.accent.color)
            }
        }
        .buttonStyle(.plain)
    }
}

// MARK: Devices

struct DevicesView: View {
    let model: DevicesModel
    @State private var renaming: DeviceEntry?
    @State private var newName = ""
    @State private var revoking: DeviceEntry?

    var body: some View {
        List {
            ForEach(model.devices) { device in
                DeviceRow(device: device)
                    .contextMenu { actions(for: device) }
                    .swipeActions { actions(for: device) }
            }
            if let error = model.error {
                ErrorText(text: error)
            }
        }
        .overlay {
            if !model.loaded && model.error == nil { ProgressView() }
        }
        .navigationTitle("Devices")
        .task { await model.load() }
        .refreshable { await model.load() }
        .alert(
            "Rename device",
            isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } }),
            presenting: renaming
        ) { device in
            TextField("Name", text: $newName)
            Button("Rename") {
                let name = newName
                Task { await model.rename(device, to: name) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("Only your devices can read the name.")
        }
        .confirmationDialog(
            "Sign out this device?",
            isPresented: Binding(get: { revoking != nil }, set: { if !$0 { revoking = nil } }),
            titleVisibility: .visible,
            presenting: revoking
        ) { device in
            Button("Sign out device", role: .destructive) { Task { await model.revoke(device) } }
        } message: { _ in
            Text("It loses access at once and deletes its copy of your vault the next time it connects.")
        }
    }

    @ViewBuilder
    private func actions(for device: DeviceEntry) -> some View {
        if device.revoked == nil {
            Button("Rename") {
                newName = device.name ?? ""
                renaming = device
            }
            if !device.isCurrent {
                Button("Sign out", role: .destructive) { revoking = device }
            }
        }
    }
}

struct DeviceRow: View {
    let device: DeviceEntry

    private var symbol: String {
        switch device.platform {
        case "ios": "iphone"
        case "android": "candybarphone"
        case "macos": "laptopcomputer"
        case "windows", "linux": "desktopcomputer"
        case "extension": "puzzlepiece.extension"
        default: "questionmark.square"
        }
    }

    private var platform: String { Self.platformName(device.platform) }

    static func platformName(_ platform: String) -> String {
        switch platform {
        case "ios": "iOS"
        case "android": "Android"
        case "macos": "macOS"
        case "windows": "Windows"
        case "linux": "Linux"
        case "extension": String(localized: "Browser extension")
        default: platform
        }
    }

    var body: some View {
        HStack(spacing: Tokens.Space.s3) {
            Image(systemName: symbol)
                .frame(width: 28)
                .foregroundStyle(Tokens.Color.accent.color)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: Tokens.Space.s2) {
                    if let name = device.name {
                        Text(verbatim: name).textStyle(Tokens.Text.bodyStrong)
                    } else {
                        Text("Unknown device").textStyle(Tokens.Text.bodyStrong)
                    }
                    if device.isCurrent {
                        Text("This device")
                            .textStyle(Tokens.Text.caption)
                            .foregroundStyle(Tokens.Color.accent.color)
                    }
                }
                Group {
                    if let revoked = device.revoked {
                        Text("\(platform) · Signed out \(revoked.formatted(date: .abbreviated, time: .omitted))")
                    } else {
                        Text("\(platform) · Last used \(device.lastSeen.formatted(date: .abbreviated, time: .shortened))")
                    }
                }
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
                Text("Added \(device.added.formatted(date: .abbreviated, time: .omitted))")
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier(device.isCurrent ? "device.current" : "device")
    }
}

// MARK: Approving a new device (D46)

struct ApprovalsView: View {
    let model: ApprovalsModel

    var body: some View {
        List {
            if let id = model.approving {
                Section {
                    if let code = model.code {
                        Text("Does the new device show this code?")
                        Text(verbatim: code)
                            .font(.secret(.largeTitle).weight(.semibold))
                            .accessibilityIdentifier("approvalCode")
                        Button("Codes match") { Task { await model.codesMatch() } }
                            .disabled(model.busy)
                            .accessibilityIdentifier("codesMatch")
                        Button("Codes don't match", role: .destructive) { Task { await model.deny(id) } }
                            .accessibilityIdentifier("codesDiffer")
                    } else {
                        HStack(spacing: Tokens.Space.s2) {
                            ProgressView()
                            Text("Waiting for the new device…")
                        }
                    }
                } footer: {
                    Text("Only approve a device you are setting up yourself. It still needs your master password.")
                }
                .task(id: id) { await model.waitForCode() }
            } else {
                if model.done {
                    Section {
                        Label("Approved. Enter your master password on the new device.", systemImage: "checkmark.seal")
                            .foregroundStyle(Tokens.Color.success.color)
                    }
                }
                Section {
                    ForEach(model.requests) { request in
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(verbatim: DeviceRow.platformName(request.platform))
                                    .textStyle(Tokens.Text.bodyStrong)
                                Text("Asked \(request.created.formatted(.relative(presentation: .named)))")
                                    .textStyle(Tokens.Text.caption)
                                    .foregroundStyle(Tokens.Color.textMuted.color)
                            }
                            Spacer()
                            Button("Approve") { Task { await model.approve(request) } }
                                .buttonStyle(.borderless)
                                .disabled(model.busy)
                                .accessibilityIdentifier("approve")
                            Button("Deny", role: .destructive) { Task { await model.deny(request.id) } }
                                .buttonStyle(.borderless)
                        }
                    }
                } footer: {
                    if model.loaded && model.requests.isEmpty {
                        Text("No device is asking. On the new device, choose Sign in with another device.")
                    }
                }
            }
            if let error = model.error {
                Section { ErrorText(text: error) }
            }
        }
        .navigationTitle("Approve a new device")
        .task { await model.load() }
        .refreshable { await model.load() }
    }
}

// MARK: Export

/// The export for the share sheet, as data: no file of the app's own.
struct ExportDocument: Transferable {
    let data: Data
    let name: String

    static var transferRepresentation: some TransferRepresentation {
        DataRepresentation(exportedContentType: .data) { $0.data }
            .suggestedFileName { $0.name }
    }
}

struct ExportView: View {
    @Bindable var model: ExportModel

    var body: some View {
        Form {
            Section {
                Picker("Format", selection: $model.choice) {
                    Text("Encrypted (.pwmx)").tag(ExportModel.Choice.encrypted)
                    Text("JSON, not encrypted").tag(ExportModel.Choice.json)
                    Text("CSV, not encrypted").tag(ExportModel.Choice.csv)
                }
                .accessibilityIdentifier("exportFormat")
            } footer: {
                switch model.choice {
                case .encrypted:
                    Text("Opens with the export password alone, without your account. Every item, folder and history entry.")
                case .json:
                    Text("Every item, folder and history entry, readable by anyone who has the file.")
                case .csv:
                    Text("Logins, notes, cards and identities as a spreadsheet, readable by anyone who has the file.")
                }
            }
            Section {
                SecureField("Master password", text: $model.masterPassword)
                    .field(.password)
                    .accessibilityIdentifier("exportMasterPassword")
            } footer: {
                Text("Every export asks for it again.")
            }
            switch model.choice {
            case .encrypted:
                Section {
                    SecureField("Export password", text: $model.exportPassword)
                        .accessibilityIdentifier("exportPassword")
                    StrengthMeter(strength: model.strength)
                    SecureField("Export password again", text: $model.confirm)
                        .accessibilityIdentifier("exportConfirm")
                } header: {
                    SectionHeader("Export password")
                }
            case .json, .csv:
                Section {
                    Toggle("I understand that anyone who gets this file can read all my passwords.", isOn: $model.understood)
                        .accessibilityIdentifier("exportUnderstood")
                }
            }
            Section {
                if let file = model.file {
                    ShareLink(
                        item: ExportDocument(data: file.value, name: model.fileName),
                        preview: SharePreview(model.fileName)
                    ) {
                        Label("Save export", systemImage: "square.and.arrow.up")
                    }
                    .accessibilityIdentifier("saveExport")
                } else {
                    Button(model.busy ? "Exporting…" : "Export") { Task { await model.export() } }
                        .disabled(!model.canExport)
                        .accessibilityIdentifier("export")
                }
                ErrorText(text: model.error)
            }
        }
        .navigationTitle("Export")
        .task(id: model.exportPassword) { await model.updateStrength() }
        .onChange(of: model.choice) { model.done() }
        .onDisappear { model.done() }
    }
}
#endif
