import Foundation
import XCTest
@testable import PWMKit

/// Against a real sync server (D41 §6): `PWM_TEST_SERVER=http://127.0.0.1:8080`.
/// Skipped when it is not set.
final class ServerTests: XCTestCase {
    var server: URL!

    override func setUpWithError() throws {
        guard let address = ProcessInfo.processInfo.environment["PWM_TEST_SERVER"] else {
            throw XCTSkip("PWM_TEST_SERVER is not set")
        }
        server = try XCTUnwrap(URL(string: address))
    }

    /// Signs `session` in to the account most tests share; returns its email
    /// and Secret Key. Only tests that need a fresh account sign up, so a
    /// run stays under the server's sign-up limit (10 requests an hour per IP).
    @discardableResult
    func logInShared(_ session: Session) async throws -> (email: String, secretKey: Redacted<String>) {
        let account = try await SharedAccount.shared.get(server: server)
        _ = try await session.logIn(
            email: account.email, password: strong, secretKey: account.secretKey, deviceName: "Test device"
        )
        return account
    }

    func testSignUpLogInOnASecondDeviceAndSync() async throws {
        let email = "swift-\(UUID().uuidString.lowercased())@example.com"
        let first = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        let secretKey = try await first.signUp(email: email, password: strong, deviceName: "iPhone")
        XCTAssertFalse(secretKey.value.isEmpty)
        let unlocked = await first.isUnlocked
        XCTAssertTrue(unlocked)

        let vault = try await XCTUnwrapAsync(try await first.vaultIds().first)
        var item = try await first.newItem(.login, title: "Example")
        item.login?.username = "ada"
        item.login?.password = "hunter2"
        item.login?.urls = [LoginURL(url: "https://example.com")]
        let itemId = try await first.save(item, in: vault)
        guard case let .synced(_, pushed, _, failed) = try await first.sync() else {
            return XCTFail("the first device did not sync")
        }
        XCTAssertEqual(pushed, 1)
        XCTAssertEqual(failed, 0)

        let second = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        guard case .synced = try await second.logIn(email: email, password: strong, secretKey: secretKey, deviceName: "Mac") else {
            return XCTFail("the second device did not sync")
        }
        let copy = try await second.item(itemId, in: vault)
        XCTAssertEqual(copy?.title, "Example")
        XCTAssertEqual(copy?.login?.password, "hunter2")
    }

    func testUnlockOfflineAfterLocking() async throws {
        let keys = MemoryKeyStore()
        let session = try Session(cachePath: nil, server: server, keys: keys)
        let email = try await logInShared(session).email
        XCTAssertNotNil(keys.get(.accessToken))
        await session.lock()
        do {
            try await session.unlock(password: Redacted("not the password"))
            XCTFail("unlocked with the wrong password")
        } catch {
            XCTAssertEqual(code(of: error), "wrong_password")
        }
        try await session.unlock(password: strong)
        let unlocked = await session.isUnlocked
        XCTAssertTrue(unlocked)
        let email2 = try await session.email()
        XCTAssertEqual(email2, email)
    }

    func testTheKitAndSigningOut() async throws {
        let keys = MemoryKeyStore()
        let session = try Session(cachePath: nil, server: server, keys: keys)
        let secretKey = try await logInShared(session).secretKey
        let kit = try await session.emergencyKit(paper: .a4)
        let text = String(decoding: kit.value, as: UTF8.self)
        XCTAssertTrue(text.hasPrefix("%PDF-1.4"))
        XCTAssertTrue(text.contains("(\(secretKey.value))"))
        // A local server is self-hosted, so its address is printed.
        XCTAssertTrue(text.contains("(Server)"))

        let deviceId = keys.get(.deviceId)
        try await session.signOut()
        let signedIn = try await session.isSignedIn()
        XCTAssertFalse(signedIn)
        let unlocked = await session.isUnlocked
        XCTAssertFalse(unlocked)
        for name in [KeyName.secretKey, .accessToken, .refreshToken, .email] {
            XCTAssertNil(keys.get(name), name.rawValue)
        }
        XCTAssertEqual(keys.get(.deviceId), deviceId, "the device id stays")
        do {
            try await session.unlock(password: strong)
            XCTFail("unlocked after signing out")
        } catch {
            XCTAssertEqual(code(of: error), "not_signed_in")
        }
        let vaults = try? await session.vaultIds()
        XCTAssertNil(vaults, "locked, and the cache is empty")
    }
}

/// Devices, health and export against the real server (D43 §9).
extension ServerTests {
    func testDevicesAreNamedAtLoginRenamedAndRevoked() async throws {
        let email = "swift-\(UUID().uuidString.lowercased())@example.com"
        let laptop = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        let secretKey = try await laptop.signUp(email: email, password: strong, deviceName: "Mac")
        let phone = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        _ = try await phone.logIn(email: email, password: strong, secretKey: secretKey, deviceName: "iPhone")

        var devices = try await phone.devices()
        XCTAssertEqual(devices.map(\.name), ["Mac", "iPhone"], "named right after login")
        XCTAssertEqual(devices.map(\.isCurrent), [false, true])
        let mac = try XCTUnwrap(devices.first)
        try await phone.renameDevice(mac.id, to: "Work Mac")
        devices = try await laptop.devices()
        XCTAssertEqual(devices.first?.name, "Work Mac")
        do {
            try await phone.renameDevice(mac.id, to: String(repeating: "x", count: 64))
            XCTFail("a long name was sent")
        } catch {
            XCTAssertEqual(code(of: error), "invalid_device_name")
        }

        let phoneId = try XCTUnwrap(devices.last?.id)
        try await laptop.revokeDevice(phoneId)
        devices = try await laptop.devices()
        XCTAssertNotNil(devices.last?.revoked)
        let result = try await phone.sync()
        XCTAssertEqual(result, .deviceRevoked)
    }

    func testHealthBreachesAndExport() async throws {
        // SHA-1 of "password123456": 98A16C09B0759E63EF7DF53592724E8EEDDB953A.
        let hibp = FakeTransport { request in
            let prefix = String(request.url.path.suffix(5))
            let body = prefix == "98A16"
                ? "C09B0759E63EF7DF53592724E8EEDDB953A:3730471\r\n"
                : "0000000000000000000000000000000000A:0\r\n"
            return HTTPResponse(status: 200, body: Data(body.utf8))
        }
        let session = try Session(cachePath: nil, server: server, keys: MemoryKeyStore(), breachTransport: hibp)
        try await logInShared(session)
        let vault = try await XCTUnwrapAsync(try await session.vaultIds().first)
        var ids: [String] = []
        for (title, password) in [("Old", "password123456"), ("Bank", "vivid-orbit-tundra-marble-42"), ("Shop", "vivid-orbit-tundra-marble-42")] {
            var item = try await session.newItem(.login, title: title)
            item.login?.password = password
            ids.append(try await session.save(item, in: vault))
        }

        let health = try await session.passwordHealth()
        XCTAssertEqual(Array(health.weak.keys), [ids[0]])
        XCTAssertEqual(health.reused.map(Set.init), [Set([ids[1], ids[2]])])
        let breached = try await session.breachedPasswords()
        XCTAssertEqual(breached, [ids[0]: 3_730_471])
        XCTAssertTrue(hibp.requests.allSatisfy { $0.url.host == "api.pwnedpasswords.com" && $0.url.path.count == 12 })
        XCTAssertTrue(hibp.requests.allSatisfy { $0.headers.contains { $0.name == "Add-Padding" && $0.value == "true" } })
        XCTAssertEqual(Set(hibp.requests.map(\.url)).count, hibp.requests.count, "each prefix once")

        do {
            _ = try await session.export(.json)
            XCTFail("exported without the master password")
        } catch {
            XCTAssertEqual(code(of: error), "export_not_authorized")
        }
        do {
            try await session.authorizeExport(password: Redacted("not the password at all"))
            XCTFail("a wrong password was accepted")
        } catch {
            XCTAssertEqual(code(of: error), "wrong_password")
        }
        try await session.authorizeExport(password: strong)
        let json = try await session.export(.json)
        XCTAssertTrue(String(decoding: json.value, as: UTF8.self).contains("password123456"))
        try await session.authorizeExport(password: strong)
        do {
            _ = try await session.export(.pwmx(password: Redacted("password")))
            XCTFail("a weak export password was accepted")
        } catch {
            XCTAssertEqual(code(of: error), "weak_export_password")
        }
        let file = try await session.export(.pwmx(password: Redacted("granite-sparrow-lattice-91")))
        XCTAssertFalse(String(decoding: file.value, as: UTF8.self).contains("password123456"))
    }
}

/// Face ID unlock through the real core and server (D45 §8).
extension ServerTests {
    final class Clock: @unchecked Sendable {
        var ms: Int64 = 1_767_225_600_000
    }

    func testFaceIDUnlocksUntilTheMasterPasswordIsDue() async throws {
        let keys = MemoryKeyStore()
        let biometrics = FakeBiometrics()
        let clock = Clock()
        let session = try Session(
            cachePath: nil, server: server, keys: keys, biometrics: biometrics, now: { clock.ms }
        )
        try await logInShared(session)
        var state = try await session.biometricState()
        XCTAssertEqual(state, .off(.faceID))

        try await session.enableBiometricUnlock(reason: "Turn on Face ID")
        state = try await session.biometricState()
        XCTAssertEqual(state, .on(.faceID))
        XCTAssertEqual(biometrics.prompts, ["Turn on Face ID"], "asked once to confirm")
        XCTAssertNotNil(keys.get(.biometricBlob))
        XCTAssertEqual(keys.get(.biometricKey), Data("wrapped".utf8).base64EncodedString(), "only the wrapped key is kept")

        await session.lock()
        try await session.unlockWithBiometrics(reason: "Unlock")
        var unlocked = await session.isUnlocked
        XCTAssertTrue(unlocked)

        // Cancelling keeps it on; the master password still works.
        await session.lock()
        biometrics.cancel = true
        do {
            try await session.unlockWithBiometrics(reason: "Unlock")
            XCTFail("unlocked after a cancel")
        } catch {
            XCTAssertEqual(code(of: error), "biometric_cancelled")
        }
        biometrics.cancel = false
        state = try await session.biometricState()
        XCTAssertEqual(state, .on(.faceID))

        // After 14 days the master password is asked once.
        clock.ms += Biometrics.masterPasswordEveryMs
        do {
            try await session.unlockWithBiometrics(reason: "Unlock")
            XCTFail("Face ID after 14 days")
        } catch {
            XCTAssertEqual(code(of: error), "master_password_due")
        }
        try await session.unlock(password: strong)
        await session.lock()
        try await session.unlockWithBiometrics(reason: "Unlock")
        unlocked = await session.isUnlocked
        XCTAssertTrue(unlocked)

        // A face added to the device: off, and the master password unlocks.
        await session.lock()
        biometrics.changed = true
        do {
            try await session.unlockWithBiometrics(reason: "Unlock")
            XCTFail("unlocked after biometrics changed")
        } catch {
            XCTAssertEqual(code(of: error), "biometric_changed")
        }
        state = try await session.biometricState()
        XCTAssertEqual(state, .off(.faceID))
        XCTAssertNil(keys.get(.biometricBlob))
        XCTAssertFalse(biometrics.hasKey)
        biometrics.changed = false
        try await session.unlock(password: strong)

        // Signing out deletes everything.
        try await session.enableBiometricUnlock(reason: "Turn on Face ID")
        try await session.signOut()
        XCTAssertNil(keys.get(.biometricBlob))
        XCTAssertNil(keys.get(.biometricKey))
        XCTAssertFalse(biometrics.hasKey)
    }

    func testFaceIDIsOffAfterSigningInAndNeedsTheHardware() async throws {
        let biometrics = FakeBiometrics()
        biometrics.available = .unavailable(reason: "No Secure Enclave.")
        let second = try Session(cachePath: nil, server: server, keys: MemoryKeyStore(), biometrics: biometrics)
        try await logInShared(second)
        let state = try await second.biometricState()
        XCTAssertEqual(state, .unavailable(reason: "No Secure Enclave."))
        do {
            try await second.enableBiometricUnlock(reason: "Turn on")
            XCTFail("turned on without the hardware")
        } catch {
            XCTAssertEqual(code(of: error), "biometric_unavailable")
        }
    }
}

/// Approving a new device between two sessions (D46).
extension ServerTests {
    final class Codes: @unchecked Sendable {
        private let lock = NSLock()
        private var value: String?
        var code: String? {
            get { lock.lock(); defer { lock.unlock() }; return value }
            set { lock.lock(); value = newValue; lock.unlock() }
        }
    }

    func testASignedInDeviceApprovesANewOne() async throws {
        let existing = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        let account = try await logInShared(existing)
        let new = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        let seen = Codes()
        let asking = Task {
            try await new.askForApproval(email: account.email) { code in seen.code = code }
        }

        var request: ApprovalEntry?
        for _ in 0..<50 where request == nil {
            request = try await existing.approvalRequests().last
            if request == nil { try await Task.sleep(for: .milliseconds(200)) }
        }
        let id = try XCTUnwrap(request?.id)
        try await existing.acceptApproval(id)
        var code: String?
        for _ in 0..<50 where code == nil {
            code = try await existing.approvalRequests().first { $0.id == id }?.code
            if code == nil { try await Task.sleep(for: .milliseconds(200)) }
        }
        XCTAssertNotNil(code)
        for _ in 0..<50 where seen.code == nil { try await Task.sleep(for: .milliseconds(100)) }
        XCTAssertEqual(seen.code, code, "the same code on both devices")

        try await existing.completeApproval(id)
        let secretKey = try await asking.value
        XCTAssertEqual(secretKey.value, account.secretKey.value)
        _ = try await new.logIn(email: account.email, password: strong, secretKey: secretKey, deviceName: "New iPhone")
        let unlocked = await new.isUnlocked
        XCTAssertTrue(unlocked)
    }
}

/// The account the server tests share, signed up once per run.
actor SharedAccount {
    static let shared = SharedAccount()
    private var account: (email: String, secretKey: Redacted<String>)?

    func get(server: URL) async throws -> (email: String, secretKey: Redacted<String>) {
        if let account { return account }
        let email = "swift-shared-\(UUID().uuidString.lowercased())@example.com"
        let session = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        let secretKey = try await session.signUp(email: email, password: strong, deviceName: "Setup")
        account = (email, secretKey)
        return (email, secretKey)
    }
}

/// `XCTUnwrap` for a value that needs `await`.
func XCTUnwrapAsync<T>(_ value: @autoclosure () async throws -> T?, file: StaticString = #filePath, line: UInt = #line) async throws -> T {
    let unwrapped = try await value()
    return try XCTUnwrap(unwrapped, file: file, line: line)
}
