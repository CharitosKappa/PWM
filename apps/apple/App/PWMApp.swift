import BackgroundTasks
import PWMUI
import SwiftUI
import UIKit
import UserNotifications

/// The iOS app: a thin shell over the shared package (D42 §2). Everything
/// it shows is in `PWMUI`. Background sync (D50 §2): iOS background
/// refresh, asked for no sooner than 15 minutes after each run, and a
/// silent push from the server.
@main
struct PWMApp: App {
    @UIApplicationDelegateAdaptor private var delegate: AppDelegate
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        let model = delegate.model
        WindowGroup {
            RootView(model: model)
                .task { await model.start() }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { BackgroundRefresh.schedule() }
        }
        .backgroundTask(.appRefresh(BackgroundRefresh.identifier)) {
            BackgroundRefresh.schedule()
            _ = await model.backgroundSync()
        }
    }
}

enum BackgroundRefresh {
    /// Also in Info.plist, `BGTaskSchedulerPermittedIdentifiers`.
    static let identifier = "pwm.sync"

    /// iOS decides when it actually runs.
    static func schedule() {
        let request = BGAppRefreshTaskRequest(identifier: identifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        try? BGTaskScheduler.shared.submit(request)
    }
}

/// Push (D50 §3): the token for the server at each launch (a silent push
/// needs no permission), a silent push starts a sync, and an approval
/// notice shows while the app is open too.
@MainActor
final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    let model = AppModel.live()

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        application.registerForRemoteNotifications()
        return true
    }

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        Task { await model.pushTokenReceived(deviceToken, sandbox: Self.sandbox) }
    }

    func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: any Error) {
        // No push (a build without the entitlement): background refresh still syncs.
    }

    func application(
        _ application: UIApplication,
        didReceiveRemoteNotification userInfo: [AnyHashable: Any]
    ) async -> UIBackgroundFetchResult {
        switch await model.backgroundSync() {
        case .newData: .newData
        case .noData: .noData
        case .failed: .failed
        }
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
