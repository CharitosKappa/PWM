import AppKit
import PWMUI
import SwiftUI
import UserNotifications

/// The macOS app (D48 §2): one window over the shared package, as on iOS.
/// The menu bar adds ⌘N for a new login, ⇧⌘L to lock, ⌘, for Settings, and
/// Sign Out…, which asks first. Background sync (D50 §2): every 15 minutes
/// while the app runs, and on a push.
@main
struct PWMMacApp: App {
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate

    private var model: AppModel { delegate.model }

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

/// Push (D50 §3) and the 15-minute timer. The system may defer the timer
/// on battery.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    let model = AppModel.live()
    private let scheduler: NSBackgroundActivityScheduler = {
        let scheduler = NSBackgroundActivityScheduler(identifier: "pwm.sync")
        scheduler.repeats = true
        scheduler.interval = 15 * 60
        scheduler.tolerance = 5 * 60
        scheduler.qualityOfService = .utility
        return scheduler
    }()

    func applicationDidFinishLaunching(_ notification: Notification) {
        UNUserNotificationCenter.current().delegate = self
        NSApplication.shared.registerForRemoteNotifications()
        let model = model
        scheduler.schedule { completion in
            // The sync runs on in the app; the scheduler only needs the start.
            Task { @MainActor in _ = await model.backgroundSync() }
            completion(.finished)
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        scheduler.invalidate()
    }

    func application(_ application: NSApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        Task { await model.pushTokenReceived(deviceToken, sandbox: Self.sandbox) }
    }

    func application(_ application: NSApplication, didFailToRegisterForRemoteNotificationsWithError error: any Error) {
        // No push (CI's ad-hoc build has no entitlement): the timer still syncs.
    }

    func application(_ application: NSApplication, didReceiveRemoteNotification userInfo: [String: Any]) {
        Task { _ = await model.backgroundSync() }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }

    /// Development builds get their tokens from APNs's sandbox.
    static var sandbox: Bool {
        #if DEBUG
        true
        #else
        false
        #endif
    }
}
