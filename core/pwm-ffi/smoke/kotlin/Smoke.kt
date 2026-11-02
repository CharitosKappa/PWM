// Kotlin smoke test of the UniFFI bindings (D31): generated with
// `just bindings`, compiled with kotlinc against JNA, run in CI on Linux.
// It checks the binding surface; the logic is tested in Rust.

import uniffi.pwm_ffi.*

fun check(ok: Boolean, what: String) {
    if (!ok) throw IllegalStateException("failed: $what")
}

fun code(block: () -> Unit): String =
    try {
        block()
        "none"
    } catch (e: PwmException.Failed) {
        e.code
    }

fun main() {
    val id = newId()
    check(Regex("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$").matches(id), "a UUIDv4 id")

    val password = generatePassword(20u, true, true, true, true, false)
    check(password.value.length == 20 && password.entropyBits > 100.0, "a 20-character password")
    check(passwordStrength("password", listOf()).score.toInt() <= 1, "a weak password scores low")
    check(code { generatePassword(3u, true, true, true, true, false) } == "invalid_item", "a too-short length is refused")

    val client = PwmClient.inMemory()
    check(!client.isUnlocked(), "a new client is locked")
    check(code { client.vaultIds() } == "locked", "locked before unlock")
    check(
        code { client.startSignup("a@example.com", "password", newId(), Platform.LINUX, "Laptop") } == "weak_password",
        "a weak master password is refused",
    )
    val step = client.startSignup("a@example.com", "correct horse battery staple 42", newId(), Platform.LINUX, "Laptop")
    check(step is Step.Send && step.request.path == "/v1/auth/register/start", "sign-up starts with register/start")
    check(code { client.networkFailed() } == "network", "a lost connection ends the sign-up")
    println("kotlin bindings: ok")
}
