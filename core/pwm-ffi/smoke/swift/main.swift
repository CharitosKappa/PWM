// Swift smoke test of the UniFFI bindings (D31): generated with
// `just bindings`, compiled with swiftc against the native library, run in
// CI on Linux. It checks the binding surface; the logic is tested in Rust.

import Foundation

func check(_ ok: Bool, _ what: String) {
    if !ok {
        print("failed: \(what)")
        exit(1)
    }
}

func code(_ block: () throws -> Void) -> String {
    do {
        try block()
        return "none"
    } catch PwmError.Failed(let code) {
        return code
    } catch {
        return "unexpected"
    }
}

let id = try newId()
check(id.count == 36 && id.split(separator: "-").count == 5, "a UUID id")

let password = try generatePassword(length: 20, lowercase: true, uppercase: true, digits: true, symbols: true, avoidLookalikes: false)
check(password.value.count == 20 && password.entropyBits > 100, "a 20-character password")
check(passwordStrength(password: "password", context: []).score <= 1, "a weak password scores low")
check(code { _ = try generatePassword(length: 3, lowercase: true, uppercase: true, digits: true, symbols: true, avoidLookalikes: false) } == "invalid_item", "a too-short length is refused")

let client = try PwmClient.inMemory()
check(!client.isUnlocked(), "a new client is locked")
check(code { _ = try client.vaultIds() } == "locked", "locked before unlock")
check(code { _ = try client.startSignup(email: "a@example.com", password: "password", deviceId: try newId(), platform: .linux, deviceName: "Laptop") } == "weak_password", "a weak master password is refused")
let step = try client.startSignup(email: "a@example.com", password: "correct horse battery staple 42", deviceId: try newId(), platform: .linux, deviceName: "Laptop")
if case let .send(request) = step {
    check(request.path == "/v1/auth/register/start", "sign-up starts with register/start")
} else {
    check(false, "sign-up sends a request")
}
check(code { _ = try client.networkFailed() } == "network", "a lost connection ends the sign-up")
print("swift bindings: ok")
