import PWMUI
import SwiftUI

/// The iOS app: a thin shell over the shared package (D42 §2). Everything
/// it shows is in `PWMUI`.
@main
struct PWMApp: App {
    @State private var model = AppModel.live()

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .task { await model.start() }
        }
    }
}
