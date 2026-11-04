// The import screen (D51 §9), from Settings.
#if canImport(SwiftUI)
import PWMDesign
import PWMKit
import SwiftUI
import UniformTypeIdentifiers

struct ImportHost: View {
    @State private var model: ImportModel

    init(settings: SettingsModel) {
        _model = State(initialValue: settings.makeImport())
    }

    var body: some View { ImportView(model: model) }
}

struct ImportView: View {
    @Bindable var model: ImportModel
    /// One file importer for both pickers: SwiftUI shows only one per view.
    @State private var picking: Picking?

    private enum Picking { case export, keyFile }

    /// What the picker offers: the export formats, and any file for a
    /// `.1pux` or `.pwmx` the system doesn't know.
    private static let types: [UTType] = [.json, .commaSeparatedText, .zip, .data]

    var body: some View {
        Form {
            Section {
                Button(model.fileName == nil ? "Choose export file" : "Choose another file") { picking = .export }
                    .accessibilityIdentifier("importChoose")
                if let name = model.fileName {
                    LabeledContent("File") { Text(verbatim: name) }
                }
            } footer: {
                Text("Bring in logins, cards, notes and folders from another password manager or a PWM export. The file is read on this device and never copied.")
            }
            if model.fileName != nil, model.done == nil {
                Section {
                    Picker("From", selection: $model.format) {
                        Text("Choose…").tag("")
                        ForEach(ImportFile.formats, id: \.id) { format in
                            Text(verbatim: format.label).tag(format.id)
                        }
                    }
                    .accessibilityIdentifier("importFormat")
                    if model.asksPassword {
                        SecureField(model.asksKeyFile ? "Master password" : "Export password", text: $model.password)
                            .accessibilityIdentifier("importPassword")
                    }
                    if model.asksKeyFile {
                        Button(model.keyFileName.map { "Key file: \($0)" } ?? "Choose key file") { picking = .keyFile }
                            .accessibilityIdentifier("importKeyFile")
                    }
                } footer: {
                    if model.asksKeyFile {
                        Text("The database's password, its key file, or both.")
                    } else if model.asksPassword {
                        Text("Only if the export was made with a password.")
                    }
                }
                if model.format == "csv", let columns = model.columns {
                    Section("Which column holds what") {
                        ForEach(ImportModel.mappedFields, id: \.label) { field in
                            Picker(field.label, selection: Binding(
                                get: { model.columnMap[keyPath: field.keyPath] ?? -1 },
                                set: { model.columnMap[keyPath: field.keyPath] = $0 < 0 ? nil : $0 }
                            )) {
                                Text("None").tag(-1)
                                ForEach(Array(columns.headers.enumerated()), id: \.offset) { index, header in
                                    Text(verbatim: header.isEmpty ? "Column \(index + 1)" : header).tag(index)
                                }
                            }
                        }
                    }
                }
                Section {
                    if let summary = model.summary {
                        Text(verbatim: "Ready to import \(ImportModel.counts(summary)).")
                        if summary.trashed > 0 {
                            Text("\(summary.trashed) of them come in the trash, as they were archived or deleted there.")
                                .textStyle(Tokens.Text.caption)
                                .foregroundStyle(Tokens.Color.textMuted.color)
                        }
                        ForEach(summary.leftOut, id: \.reason) { left in
                            Text(verbatim: ImportModel.leftOut(left))
                                .textStyle(Tokens.Text.caption)
                                .foregroundStyle(Tokens.Color.textMuted.color)
                        }
                        Button(model.working ? "Importing…" : "Import") { Task { await model.importNow() } }
                            .disabled(model.working)
                            .accessibilityIdentifier("importNow")
                    } else {
                        Button(model.working ? "Reading…" : "Check file") { Task { await model.check() } }
                            .disabled(!model.canCheck)
                            .accessibilityIdentifier("importCheck")
                    }
                    ErrorText(text: model.error)
                }
            }
            if let done = model.done {
                Section {
                    Text(verbatim: "Imported \(ImportModel.counts(done)).")
                        .accessibilityIdentifier("importDone")
                    ForEach(done.leftOut, id: \.reason) { left in
                        Text(verbatim: ImportModel.leftOut(left))
                            .textStyle(Tokens.Text.caption)
                            .foregroundStyle(Tokens.Color.textMuted.color)
                    }
                }
                if model.plaintext {
                    Section {
                        if model.deleted {
                            Text("The export file is deleted.")
                        } else {
                            Text("The export file holds your passwords in plain text. Delete it now.")
                                .foregroundStyle(Tokens.Color.danger.color)
                            if model.canDelete {
                                Button("Delete export file", role: .destructive) { model.deleteFile() }
                                    .accessibilityIdentifier("importDeleteFile")
                            }
                        }
                        ErrorText(text: model.error)
                    }
                }
            }
        }
        .navigationTitle("Import")
        .fileImporter(
            isPresented: Binding(get: { picking != nil }, set: { if !$0 { picking = nil } }),
            allowedContentTypes: picking == .keyFile ? [.data] : Self.types
        ) { result in
            let target = picking
            picking = nil
            guard case let .success(url) = result else { return }
            switch target {
            case .keyFile: model.pickedKeyFile(url)
            default: Task { await model.picked(url) }
            }
        }
        .onDisappear { Task { await model.close() } }
    }
}
#endif
