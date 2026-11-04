import Foundation
import PWMKit

// The native-messaging helper (D48 §5): Chrome, Edge, Brave, Opera and
// Firefox start it with the caller in its arguments, then send framed JSON
// on stdin. It answers each message on stdout and exits when stdin closes.
// It never writes anything else there, and logs nothing about the messages.

let caller = BrowserUnlock.caller(arguments: CommandLine.arguments)
let helper = BrowserHelperPlaces.helper()
let input = FileHandle.standardInput
let output = FileHandle.standardOutput
var buffer = Data()

reading: while true {
    let chunk = input.availableData
    if chunk.isEmpty { break }
    buffer.append(chunk)
    while true {
        let next: (message: Data, rest: Data)?
        do {
            next = try NativeMessaging.unframe(buffer)
        } catch {
            break reading
        }
        guard let next else { break }
        buffer = next.rest
        let reply = await helper.handle(next.message, caller: caller)
        output.write(NativeMessaging.frame(reply))
    }
}
