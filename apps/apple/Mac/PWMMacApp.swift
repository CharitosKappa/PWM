import PWMUI
import SwiftUI

/// The macOS app (D48 §2): one window over the shared package, as on iOS.
/// The menu bar adds ⌘N for a new login and ⇧⌘L to lock.
@main
struct PWMMacApp: App {
    @State private var model = AppModel.live()

    var body: some Scene {
        Window("PWM", id: "main") {
            RootView(model: model)
                .frame(minWidth: 820, minHeight: 540)
                .task { await model.start() }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Login") { model.vault?.requestNew(.login) }
                    .keyboardShortcut("n")
                    .disabled(model.vault == nil)
            }
            CommandGroup(after: .newItem) {
                Button("Lock Vault") { Task { await model.lock() } }
                    .keyboardShortcut("l", modifiers: [.command, .shift])
                    .disabled(model.vault == nil)
            }
        }
    }
}
