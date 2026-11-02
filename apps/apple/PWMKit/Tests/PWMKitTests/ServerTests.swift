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
        guard case .synced = try await second.logIn(email: email, password: strong, secretKey: secretKey) else {
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

/// `XCTUnwrap` for a value that needs `await`.
func XCTUnwrapAsync<T>(_ value: @autoclosure () async throws -> T?, file: StaticString = #filePath, line: UInt = #line) async throws -> T {
    let unwrapped = try await value()
    return try XCTUnwrap(unwrapped, file: file, line: line)
}
