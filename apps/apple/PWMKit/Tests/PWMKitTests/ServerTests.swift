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

    /// The server's answer to refreshing with `token`.
    func refreshStatus(_ token: String) async throws -> UInt16 {
        let body = try JSONSerialization.data(withJSONObject: ["refresh_token": token])
        let request = HTTPRequest(
            method: "POST", url: server.appendingPathComponent("v1/auth/refresh"),
            headers: [(name: "content-type", value: "application/json")], body: body
        )
        return try await URLSessionTransport().send(request).status
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

    /// D50: a locked device syncs with its tokens only, uploading what it
    /// had queued, and sends its push token while locked too; then D51's
    /// import. One login, as the server limits them.
    func testALockedBackgroundSyncThePushTokenAndImport() async throws {
        let session = try Session(cachePath: nil, server: server, keys: MemoryKeyStore())
        try await logInShared(session)
        let vault = try await XCTUnwrapAsync(try await session.vaultIds().first)
        let itemId = try await session.save(try await session.newItem(.note, title: "Locked sync"), in: vault)
        await session.lock()

        guard case let .synced(_, pushed, _, failed) = try await session.backgroundSync() else {
            return XCTFail("the locked device did not sync")
        }
        XCTAssertEqual(pushed, 1)
        XCTAssertEqual(failed, 0)
        let stillLocked = await session.isUnlocked
        XCTAssertFalse(stillLocked)
        try await session.setPushToken(Data(repeating: 0xab, count: 32), sandbox: true)

        try await session.unlock(password: strong)
        guard case let .synced(_, pushedAgain, _, _) = try await session.sync() else {
            return XCTFail("the unlocked device did not sync")
        }
        XCTAssertEqual(pushedAgain, 0, "it went up while locked")
        let copy = try await session.item(itemId, in: vault)
        XCTAssertEqual(copy?.title, "Locked sync")
        try await session.setPushToken(nil, sandbox: true)

        // D51, on the same login: a browser's CSV comes in through the core,
        // is saved and syncs; the same file again adds nothing.
        let title = "Imported \(UUID().uuidString.prefix(8))"
        // A strong, unique password: the account is shared, and the health
        // test counts weak and reused ones.
        let password = "lantern-\(UUID().uuidString.prefix(8))-orbit-velvet-42"
        let file = Redacted(Data("name,url,username,password\n\(title),https://import.example,ada,\(password)\n".utf8))
        XCTAssertEqual(session.guessImport(fileName: "passwords.csv", data: file)?.format, "csv")
        XCTAssertEqual(try session.importColumns(file).suggested.password, 3)
        let preview = try await session.prepareImport(format: "csv", data: file, password: nil, keyFile: nil, columns: nil)
        XCTAssertEqual(preview.logins, 1)
        let done = try await session.finishImport()
        XCTAssertEqual(done.logins, 1)
        let titles = try await session.items(in: vault).map(\.title)
        XCTAssertTrue(titles.contains(title))
        let again = try await session.prepareImport(format: "csv", data: file, password: nil, keyFile: nil, columns: nil)
        XCTAssertEqual(again.logins, 0)
        XCTAssertEqual(again.leftOut, [ImportSummary.LeftOut(reason: "duplicates", count: 1)])
        await session.cancelImport()
    }

    /// One login for both, to stay under the login limit (30 requests a
    /// minute per IP): only logout goes unanswered.
    func testUnlockOfflineAfterLockingAndSignOutPastADeadServer() async throws {
        let keys = MemoryKeyStore()
        let session = try Session(cachePath: nil, server: server, keys: keys, transport: NoLogoutAnswer())
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

        let kept = try XCTUnwrap(keys.get(.refreshToken))
        let start = Date()
        try await session.signOut()
        XCTAssertLessThan(Date().timeIntervalSince(start), 10)
        XCTAssertNil(keys.get(.refreshToken))
        // Offline at sign-out, the session stays on the server until it expires (D61 §4).
        let status = try await refreshStatus(kept)
        XCTAssertEqual(status, 200)
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
        let ended = try XCTUnwrap(keys.get(.refreshToken))
        try await session.signOut()
        // The session is over on the server too (D61).
        let status = try await refreshStatus(ended)
        XCTAssertEqual(status, 401)
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

    /// The suggestions iOS gets (D47 §5), what the AutoFill extension fills
    /// and signs (§4, §6), and Face ID after the move to the shared places.
    func testSuggestionsAndWhatTheExtensionFills() async throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "pwm-suggest-\(UUID().uuidString)"))
        let store = FakeSuggestionStore()
        let suggestions = Suggestions(store: store, defaults: defaults)
        let keys = MemoryKeyStore()
        let session = try Session(
            cachePath: nil, server: server, keys: keys, biometrics: FakeBiometrics(), suggestions: suggestions
        )
        try await logInShared(session)
        let domain = "example-\(UUID().uuidString.prefix(8).lowercased()).com"
        let vaultId = try await XCTUnwrapAsync(try await session.vaultIds().first)
        var item = try await session.newItem(.login, title: "Shop")
        var login = Login()
        login.username = "ada"
        login.password = "hunter2-secret"
        login.urls = [LoginURL(url: "https://shop.\(domain)/login")]
        item.login = login
        let itemId = try await session.save(item, in: vaultId)
        let record = Suggestion.record(vaultId: vaultId, itemId: itemId)

        await suggestions.settle()
        var last = await store.lists.last ?? []
        let password = try XCTUnwrap(last.first { $0.record == record && $0.kind == .password })
        XCTAssertEqual(password.service, domain, "the registrable domain")
        XCTAssertEqual(password.user, "ada")
        XCTAssertFalse(String(describing: last).contains("hunter2"), "never a password")

        let choices = try await session.fillChoices(for: ["shop.\(domain)"])
        XCTAssertEqual(choices.map(\.itemId), [itemId])
        let elsewhere = try await session.fillChoices(for: ["other-\(domain)"])
        XCTAssertTrue(elsewhere.isEmpty)
        let all = try await session.allFillChoices()
        XCTAssertTrue(all.contains { $0.itemId == itemId })
        let credential = try await session.fillCredential(itemId, in: vaultId)
        XCTAssertEqual(credential.username, "ada")
        XCTAssertEqual(credential.password.value, "hunter2-secret")
        XCTAssertFalse(credential.hasCode)

        // A passkey made from what iOS hands over joins the login.
        let made = try await session.registerPasskey(rpId: domain, userName: "ada", userHandle: Data("handle".utf8))
        XCTAssertEqual(made.credentialId.count, 16)
        XCTAssertFalse(made.attestationObject.isEmpty)
        await suggestions.settle()
        last = await store.lists.last ?? []
        let passkey = try XCTUnwrap(last.first { $0.record == record && $0.kind == .passkey })
        XCTAssertEqual(passkey.service, domain)
        XCTAssertEqual(passkey.credentialId, made.credentialId)
        XCTAssertEqual(passkey.userHandle, Data("handle".utf8))
        let passkeys = try await session.passkeyChoices(rpId: domain, allowed: [])
        XCTAssertEqual(passkeys.map(\.credentialId), [made.credentialId])
        let unlisted = try await session.passkeyChoices(rpId: domain, allowed: [Data([1])])
        XCTAssertTrue(unlisted.isEmpty)
        let signed = try await session.signWithPasskey(
            try XCTUnwrap(passkeys.first), rpId: domain, clientDataHash: Data(repeating: 7, count: 32)
        )
        XCTAssertEqual(signed.authenticatorData.count, 37)
        XCTAssertEqual(signed.userHandle, Data("handle".utf8))
        XCTAssertFalse(signed.signature.isEmpty)

        // The switch.
        await session.setSuggestions(on: false)
        await suggestions.settle()
        last = await store.lists.last ?? [Suggestion(kind: .password, service: "", user: "", record: "")]
        XCTAssertEqual(last, [])
        await session.setSuggestions(on: true)
        await suggestions.settle()
        last = await store.lists.last ?? []
        XCTAssertTrue(last.contains { $0.record == record })

        // Face ID moved without its key: the password unlock makes it again.
        try await session.enableBiometricUnlock(reason: "Turn on Face ID")
        keys.delete(.biometricKey)
        await session.lock()
        do {
            try await session.unlockWithBiometrics(reason: "Fill")
            XCTFail("unlocked without the key")
        } catch {
            XCTAssertEqual(code(of: error), "master_password_due")
        }
        let state = try await session.biometricState()
        XCTAssertEqual(state, .on(.faceID), "still on")
        try await session.unlock(password: strong)
        XCTAssertNotNil(keys.get(.biometricKey))
        await session.lock()
        try await session.unlockWithBiometrics(reason: "Fill")

        // Trash takes it out; signing out empties the list.
        try await session.setTrashed(itemId, in: vaultId, trashed: true)
        await suggestions.settle()
        last = await store.lists.last ?? []
        XCTAssertFalse(last.contains { $0.record == record })
        try await session.signOut()
        await suggestions.settle()
        last = await store.lists.last ?? [Suggestion(kind: .password, service: "", user: "", record: "")]
        XCTAssertEqual(last, [])
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

/// The real server, except that logout never answers.
private struct NoLogoutAnswer: HTTPTransport {
    let real = URLSessionTransport()

    func send(_ request: HTTPRequest) async throws -> HTTPResponse {
        if request.url.path.hasSuffix("/v1/auth/logout") {
            try await Task.sleep(nanoseconds: 60_000_000_000)
        }
        return try await real.send(request)
    }
}
