import Foundation
import PWMKit
import SafariServices

/// The Safari Web Extension's native side (D48 §4, §5). The extension itself
/// is the shared web build (`extension/dist/safari/`), copied in at build
/// time. Its native messages are Touch ID unlock requests, answered by the
/// same helper as the other browsers' native-messaging host.
final class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        let item = context.inputItems.first as? NSExtensionItem
        let message = item?.userInfo?[SFExtensionMessageKey]
        let request = message.flatMap { try? JSONSerialization.data(withJSONObject: $0) } ?? Data()
        let reply = Reply(context: context)
        Task {
            let answer = await BrowserHelperPlaces.helper().handle(request, caller: BrowserUnlock.safariCaller)
            reply.send(answer)
        }
    }

    /// Holds the context until the answer is ready; used once.
    private final class Reply: @unchecked Sendable {
        let context: NSExtensionContext

        init(context: NSExtensionContext) {
            self.context = context
        }

        func send(_ answer: Data) {
            let response = NSExtensionItem()
            response.userInfo = [SFExtensionMessageKey: (try? JSONSerialization.jsonObject(with: answer)) ?? [:]]
            context.completeRequest(returningItems: [response], completionHandler: nil)
        }
    }
}
