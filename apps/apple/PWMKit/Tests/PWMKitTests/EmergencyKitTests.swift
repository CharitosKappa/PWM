import Foundation
import XCTest
@testable import PWMKit

final class EmergencyKitTests: XCTestCase {
    let key = "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA"

    func testTheKitIsAPDFOnTheLocalesPaper() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Pacific/Auckland")!
        // 2026-11-02 11:00 UTC is already the 3rd in Auckland.
        let created = Date(timeIntervalSince1970: 1_793_617_200)
        let pdf = try EmergencyKit.pdf(
            email: "ada@example.com", secretKey: key, server: URL(string: "https://vault.example.org")!,
            accountId: "0f8e2f4c-3a1b-4d5e-9f60-7a8b9c0d1e2f", created: created, paper: .letter, calendar: calendar
        )
        let text = String(decoding: pdf.value, as: UTF8.self)
        XCTAssertTrue(text.hasPrefix("%PDF-1.4"))
        XCTAssertTrue(text.contains("/MediaBox [0 0 612 792]"))
        XCTAssertTrue(text.contains("2026-11-03"))
        XCTAssertTrue(text.contains("(https://vault.example.org)"))
        XCTAssertEqual("\(pdf)", "<redacted>")
    }

    func testTheHostedServerIsNotPrinted() throws {
        let pdf = try EmergencyKit.pdf(
            email: "ada@example.com", secretKey: key, server: Server.hosted,
            accountId: "0f8e2f4c-3a1b-4d5e-9f60-7a8b9c0d1e2f", created: Date(), paper: .a4
        )
        let text = String(decoding: pdf.value, as: UTF8.self)
        XCTAssertFalse(text.contains("(Server)"))
        XCTAssertTrue(text.contains("/MediaBox [0 0 595 842]"))
    }

    func testPaperFollowsTheRegion() {
        XCTAssertEqual(EmergencyKit.Paper.of(Locale(identifier: "en_US")), .letter)
        XCTAssertEqual(EmergencyKit.Paper.of(Locale(identifier: "fr_CA")), .letter)
        XCTAssertEqual(EmergencyKit.Paper.of(Locale(identifier: "en_GB")), .a4)
        XCTAssertEqual(EmergencyKit.Paper.of(Locale(identifier: "el_GR")), .a4)
    }

    func testScanningAKit() throws {
        let scanned = try EmergencyKit.scan("pwm-kit:1?email=ada%40example.com&secret-key=\(key)&server=http%3A%2F%2Flocalhost%3A8080")
        XCTAssertEqual(scanned.email, "ada@example.com")
        XCTAssertEqual(scanned.secretKey.value, key)
        XCTAssertEqual(scanned.server, URL(string: "http://localhost:8080"))
        XCTAssertFalse("\(scanned)".contains("QZXQ5T"), "the Secret Key never prints")
        XCTAssertThrowsError(try EmergencyKit.scan("hello")) { XCTAssertEqual(code(of: $0), "not_a_kit") }
    }

    func testAKitNeedsASignedInAccount() async throws {
        let session = try Session(cachePath: nil, server: Server.hosted, keys: MemoryKeyStore(), transport: FakeTransport { _ in HTTPResponse(status: 500) })
        do {
            _ = try await session.emergencyKit()
            XCTFail("a kit without an account")
        } catch {
            XCTAssertEqual(code(of: error), "not_signed_in")
        }
        let signedIn = try await session.isSignedIn()
        XCTAssertFalse(signedIn)
    }
}
