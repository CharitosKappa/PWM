import Foundation
import XCTest
@testable import PWMKit

/// The core's D18 fixture, read and edited through `Item`.
final class ItemTests: XCTestCase {
    static func fixture() throws -> [String] {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let data = try Data(contentsOf: root.appendingPathComponent("core/pwm-vault/tests/fixtures/items_v1.json"))
        let items = try JSONDecoder().decode([JSONValue].self, from: data)
        return try items.map { try $0.text() }
    }

    func testReadsEveryTypeOfTheFixture() throws {
        let items = try Self.fixture().map(Item.init(json:))
        XCTAssertEqual(items.map(\.type), ["login", "note", "card", "identity", "folder", "login", "wifi", "login", items[8].type])

        let login = items[0]
        XCTAssertEqual(login.kind, .login)
        XCTAssertEqual(login.title, "Example")
        XCTAssertTrue(login.favorite)
        XCTAssertEqual(login.tags, ["work", "email"])
        XCTAssertEqual(login.folderId, "0f0e0d0c-0b0a-4908-8706-050403020100")
        XCTAssertEqual(login.createdAtMs, 1_767_225_600_000)
        XCTAssertEqual(login.login?.username, "alice@example.com")
        XCTAssertEqual(login.login?.password, "hunter2")
        XCTAssertEqual(login.login?.urls.map(\.match), ["domain", "starts_with"])
        XCTAssertEqual(login.login?.passkeyCount, 1)
        XCTAssertNotNil(login.login?.totp)
        XCTAssertTrue(login.isEditable)
    }

    func testKeepsUnknownFieldsThroughAnEdit() throws {
        var item = try Item(json: try Self.fixture()[5])
        item.title = "Renamed"
        var login = try XCTUnwrap(item.login)
        login.password = "new password"
        var url = try XCTUnwrap(login.urls.first)
        url.url = "https://example.com"
        login.urls = [url]
        item.login = login

        let again = try Item(json: try item.text())
        XCTAssertEqual(again.title, "Renamed")
        XCTAssertEqual(again.login?.password, "new password")
        XCTAssertEqual(again.json["future_top_level"], .object(["kept": .array([.int(1), .int(2), .int(3)])]))
        XCTAssertEqual(again.login?.json["future_login_field"], .string("keep me"))
        XCTAssertEqual(again.login?.urls.first?.json["future_url_option"], .string("x"))
        XCTAssertEqual(again.login?.urls.first?.match, "fuzzy")
        XCTAssertEqual(again.json["custom_fields"]?.array?.first?.object?["future_field_flag"], .bool(true))
    }

    func testNewerItemsAreReadOnly() throws {
        let items = try Self.fixture().map(Item.init(json:))
        XCTAssertNil(items[6].kind, "a type from a newer app version")
        XCTAssertFalse(items[6].isEditable)
        XCTAssertEqual(items[7].schemaVersion, 2)
        XCTAssertFalse(items[7].isEditable)
    }

    func testANewItem() throws {
        var item = Item(kind: .login, title: "Bank", id: "item-1", nowMs: 42)
        item.login?.username = "ada"
        item.login?.urls = [LoginURL(url: "https://bank.example")]
        let json = try JSONValue.parse(try item.text()).object
        XCTAssertEqual(json?["v"], .int(1))
        XCTAssertEqual(json?["type"], .string("login"))
        XCTAssertEqual(json?["id"], .string("item-1"))
        XCTAssertEqual(json?["created_at"], .int(42))
        XCTAssertEqual(json?["login"]?.object?["username"], .string("ada"))
    }

    func testNeverPrintsValues() throws {
        let item = try Item(json: try Self.fixture()[0])
        var dumped = ""
        dump(item, to: &dumped)
        for text in ["\(item)", String(reflecting: item), "\(item.json)", "\(item.login!)", dumped] {
            XCTAssertFalse(text.contains("hunter2"), text)
            XCTAssertFalse(text.contains("alice"), text)
        }
        XCTAssertThrowsError(try Item(json: "not json")) { error in
            XCTAssertEqual((error as? PWMError)?.code, "invalid_item")
        }
    }
}
