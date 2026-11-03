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
        let email = "swift-\(UUID().uuidString.lowercased())@example.com"
        let keys = MemoryKeyStore()
        let session = try Session(cachePath: nil, server: server, keys: keys)
        _ = try await session.signUp(email: email, password: strong, deviceName: "Mac")
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
        let email = "swift-\(UUID().uuidString.lowercased())@example.com"
        let keys = MemoryKeyStore()
        let session = try Session(cachePath: nil, server: server, keys: keys)
        let secretKey = try await session.signUp(email: email, password: strong, deviceName: "iPhone")
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
        let email = "swift-\(UUID().uuidString.lowercased())@example.com"
        // SHA-1 of "password123456": 98A16C09B0759E63EF7DF53592724E8EEDDB953A.
        let hibp = FakeTransport { request in
            let prefix = String(request.url.path.suffix(5))
            let body = prefix == "98A16"
                ? "C09B0759E63EF7DF53592724E8EEDDB953A:3730471\r\n"
                : "0000000000000000000000000000000000A:0\r\n"
            return HTTPResponse(status: 200, body: Data(body.utf8))
        }
        let session = try Session(cachePath: nil, server: server, keys: MemoryKeyStore(), breachTransport: hibp)
        _ = try await session.signUp(email: email, password: strong, deviceName: "iPhone")
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

/// `XCTUnwrap` for a value that needs `await`.
func XCTUnwrapAsync<T>(_ value: @autoclosure () async throws -> T?, file: StaticString = #filePath, line: UInt = #line) async throws -> T {
    let unwrapped = try await value()
    return try XCTUnwrap(unwrapped, file: file, line: line)
}
