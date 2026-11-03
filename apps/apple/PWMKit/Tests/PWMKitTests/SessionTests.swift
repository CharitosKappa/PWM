import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import XCTest
@testable import PWMKit

/// Answers every request the same way and records what was asked.
final class FakeTransport: HTTPTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var sent: [HTTPRequest] = []
    let answer: @Sendable (HTTPRequest) throws -> HTTPResponse

    init(answer: @escaping @Sendable (HTTPRequest) throws -> HTTPResponse) {
        self.answer = answer
    }

    var requests: [HTTPRequest] {
        lock.lock()
        defer { lock.unlock() }
        return sent
    }

    private func record(_ request: HTTPRequest) {
        lock.lock()
        defer { lock.unlock() }
        sent.append(request)
    }

    func send(_ request: HTTPRequest) async throws -> HTTPResponse {
        record(request)
        return try answer(request)
    }
}

let strong = Redacted("correct horse battery staple 42")

func code(of error: Error) -> String? {
    (error as? PWMError)?.code
}

final class SessionTests: XCTestCase {
    func session(_ transport: FakeTransport, keys: KeyStore = MemoryKeyStore()) throws -> Session {
        try Session(cachePath: nil, server: URL(string: "https://pwm.test/")!, keys: keys, transport: transport)
    }

    func testALostConnectionEndsTheSignUp() async throws {
        let transport = FakeTransport { _ in throw URLError(.notConnectedToInternet) }
        let session = try session(transport)
        do {
            _ = try await session.signUp(email: "ada@example.com", password: strong, deviceName: "iPhone")
            XCTFail("signed up without a server")
        } catch {
            XCTAssertEqual(code(of: error), "network")
        }
        // Joined to the server's base URL without a double slash.
        XCTAssertEqual(transport.requests.first?.url.absoluteString, "https://pwm.test/v1/auth/register/start")
        XCTAssertEqual(transport.requests.first?.method, "POST")
    }

    func testAWeakPasswordNeverReachesTheServer() async throws {
        let transport = FakeTransport { _ in HTTPResponse(status: 500) }
        let session = try session(transport)
        do {
            _ = try await session.signUp(email: "ada@example.com", password: Redacted("password"), deviceName: "iPhone")
            XCTFail("signed up with a weak password")
        } catch {
            XCTAssertEqual(code(of: error), "weak_password")
        }
        XCTAssertTrue(transport.requests.isEmpty)
    }

    func testKeepsOneDeviceIdPerInstall() async throws {
        let keys = MemoryKeyStore()
        let transport = FakeTransport { _ in throw URLError(.timedOut) }
        let session = try session(transport, keys: keys)
        _ = try? await session.signUp(email: "ada@example.com", password: strong, deviceName: "iPhone")
        let first = try XCTUnwrap(keys.get(.deviceId))
        _ = try? await session.signUp(email: "ada@example.com", password: strong, deviceName: "iPhone")
        XCTAssertEqual(keys.get(.deviceId), first)
    }

    func testUnlockNeedsASignedInDevice() async throws {
        let session = try session(FakeTransport { _ in HTTPResponse(status: 500) })
        do {
            try await session.unlock(password: strong)
            XCTFail("unlocked without an account")
        } catch {
            XCTAssertEqual(code(of: error), "not_signed_in")
        }
        let locked = await session.isUnlocked
        XCTAssertFalse(locked)
    }

    func testErrorsCarryCodesAndPlainMessagesOnly() {
        let error = PWMError(code: "wrong_password")
        XCTAssertEqual("\(error)", "PWMError(wrong_password)")
        XCTAssertEqual(error.message, "Wrong master password.")
        XCTAssertEqual(PWMError(code: "something_new").message, "Something went wrong. Try again.")
    }

    func testRedactedNeverPrintsItsValue() {
        let secret = Redacted("A3-XYZ-secret")
        var dumped = ""
        dump(secret, to: &dumped)
        for text in ["\(secret)", String(reflecting: secret), dumped] {
            XCTAssertFalse(text.contains("XYZ"), text)
        }
        XCTAssertEqual(secret.value, "A3-XYZ-secret")
    }

    func testGenerator() throws {
        let password = try Generator.password()
        XCTAssertEqual(password.value.value.count, 20)
        XCTAssertGreaterThan(password.entropyBits, 100)
        XCTAssertEqual(try Generator.passphrase().value.value.split(separator: "-").count, 5)
        XCTAssertLessThanOrEqual(Generator.strength(of: Redacted("password")).score, 1)
        XCTAssertThrowsError(try Generator.password(length: 3))
    }
}

/// A Secure Enclave stand-in (D45): it keeps the secret in memory and can
/// be cancelled, "change biometrics", or be missing.
final class FakeBiometrics: BiometricKeyStore, @unchecked Sendable {
    private let lock = NSLock()
    private var secret: Data?
    var available: BiometricAvailability = .available(.faceID)
    var cancel = false
    var changed = false
    private(set) var prompts: [String] = []

    func availability() -> BiometricAvailability { locked { available } }

    func wrap(_ secret: Redacted<Data>) throws -> Data {
        locked { self.secret = secret.value }
        return Data("wrapped".utf8)
    }

    func unwrap(_ wrapped: Data, reason: String) async throws -> Redacted<Data> {
        try locked {
            prompts.append(reason)
            if cancel { throw PWMError(code: "biometric_cancelled") }
            guard !changed, wrapped == Data("wrapped".utf8), let secret else {
                throw PWMError(code: "biometric_changed")
            }
            return Redacted(secret)
        }
    }

    func delete() throws { locked { secret = nil } }

    var hasKey: Bool { locked { secret != nil } }

    private func locked<T>(_ body: () throws -> T) rethrows -> T {
        lock.lock()
        defer { lock.unlock() }
        return try body()
    }
}
