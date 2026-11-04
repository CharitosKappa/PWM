import PWMUI
import SwiftUI

/// The macOS app (D48 §2): one window over the shared package, as on iOS.
/// The menu bar adds ⌘N for a new login, ⇧⌘L to lock, ⌘, for Settings, and
/// Sign Out…, which asks first.
@main
struct PWMMacApp: App {
    @State private var model = AppModel.live()

    var body: some Scene {
        Window("PWM", id: "main") {
            RootView(model: model)
                .frame(minWidth: 820, minHeight: 540)
                .task { await model.start() }
        }
        // Sized from the content's minimum, not its full height (a long
        // Settings form would make the window taller than the screen), and
        // small enough to sit above the Dock on a 768-point screen.
        .windowResizability(.contentMinSize)
        .defaultSize(width: 1000, height: 600)
        .commands {
            CommandGroup(replacing: .appSettings) {
                Button("Settings…") { model.requestSettings() }
                    .keyboardShortcut(",")
                    .disabled(model.vault == nil)
            }
            CommandGroup(replacing: .newItem) {
                Button("New Login") { model.vault?.requestNew(.login) }
                    .keyboardShortcut("n")
                    .disabled(model.vault == nil)
            }
            CommandGroup(after: .newItem) {
                Button("Lock Vault") { Task { await model.lock() } }
                    .keyboardShortcut("l", modifiers: [.command, .shift])
                    .disabled(model.vault == nil)
                Button("Sign Out…") { model.askSignOut() }
                    .disabled(model.vault == nil)
            }
        }
    }
}
