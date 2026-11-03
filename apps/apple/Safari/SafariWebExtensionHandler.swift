import Foundation

/// The Safari Web Extension's native side (D48 §4). The extension itself is
/// the shared web build (`extension/dist/safari/`), copied in at build time.
/// It sends no native messages yet; Touch ID unlock comes with M5.12, which
/// answers here.
final class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        context.completeRequest(returningItems: [], completionHandler: nil)
    }
}
