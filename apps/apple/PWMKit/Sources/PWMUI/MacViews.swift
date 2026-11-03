// The macOS layout (D48 §2): one sidebar with the vault's sections, then
// Generator and Settings; the item list and the item beside it.
#if canImport(SwiftUI) && os(macOS)
import PWMDesign
import PWMKit
import SwiftUI

struct MacMainView: View {
    let app: AppModel
    let vault: VaultModel
    let generator: GeneratorModel
    let settings: SettingsModel

    enum Row: Hashable {
        case section(VaultModel.Section)
        case generator, settings
    }

    @State private var row = Row.section(.all)
    @State private var selection: String?
    @State private var editing: ItemEditModel?

    private var sidebar: Binding<Row?> {
        Binding(get: { row }, set: { new in
            guard let new else { return }
            row = new
            if case let .section(section) = new { vault.section = section }
        })
    }

    var body: some View {
        NavigationSplitView {
            List(selection: sidebar) {
                Section("Vault") {
                    Label("All items", systemImage: "tray.full").tag(Row.section(.all))
                    Label("Favorites", systemImage: "star").tag(Row.section(.favorites))
                    ForEach(VaultModel.kinds, id: \.self) { kind in
                        Label(kind.sectionLabel, systemImage: kind.symbol).tag(Row.section(.kind(kind)))
                    }
                }
                if !vault.folders.isEmpty {
                    Section("Folders") {
                        ForEach(vault.folders) { folder in
                            Label(folder.item.title, systemImage: "folder").tag(Row.section(.folder(folder.id)))
                        }
                    }
                }
                Label("Trash", systemImage: "trash").tag(Row.section(.trash))
                Section {
                    Label("Generator", systemImage: "dice").tag(Row.generator)
                        .accessibilityIdentifier("sidebar.generator")
                    Label("Settings", systemImage: "gearshape").tag(Row.settings)
                        .badge(settings.pendingApprovals)
                        .accessibilityIdentifier("sidebar.settings")
                }
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 210)
        } content: {
            switch row {
            case .section:
                ItemList(model: vault, selection: $selection, editing: $editing)
                    .navigationSplitViewColumnWidth(min: 260, ideal: 300)
            case .generator:
                GeneratorView(model: generator, onUse: nil)
            case .settings:
                SettingsView(model: settings)
            }
        } detail: {
            if case .section = row, let entry = vault.entry(selection) {
                DetailHost(entry: entry, vault: vault) {
                    editing = vault.edit(entry)
                }
                .id(entry.id + String(entry.item.updatedAtMs ?? 0) + String(entry.item.trashedAtMs ?? 0))
            } else if case .section = row {
                Text("Select an item")
                    .foregroundStyle(Tokens.Color.textMuted.color)
            }
        }
        .task {
            await vault.load()
            await vault.refresh()
        }
        .onChange(of: app.settingsRequests) { row = .settings }
        // ⌘N from another pane, or from the trash: back to the list.
        .onChange(of: vault.pendingNew) { _, kind in
            guard kind != nil else { return }
            row = .section(vault.section)
        }
        .sheet(item: $editing) { edit in
            NavigationStack { ItemEditView(model: edit) }
                .frame(minWidth: 480, minHeight: 560)
        }
    }
}
#endif
