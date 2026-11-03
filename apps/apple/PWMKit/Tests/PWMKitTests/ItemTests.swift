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

    func testCardsIdentitiesAndCustomFields() throws {
        let items = try Self.fixture().map(Item.init(json:))
        let card = try XCTUnwrap(items[2].card)
        XCTAssertEqual(card.cardholder, "Alice Example")
        XCTAssertEqual(card.number, "4111 1111 1111 1111")
        XCTAssertEqual(card.lastFour, "1111")
        XCTAssertEqual(card.expiryMonth, 12)
        XCTAssertEqual(card.expiryYear, 2030)
        XCTAssertEqual(card.securityCode, "123")
        XCTAssertEqual(card.brand, "visa")

        let identity = try XCTUnwrap(items[3].identity)
        XCTAssertEqual(identity.firstName, "Alice")
        XCTAssertEqual(identity.lastName, "Example")
        XCTAssertEqual(identity.addressLines, ["1 Main Street", "Flat 2"])
        XCTAssertEqual(identity.country, "GR")

        XCTAssertEqual(items[0].customFields.map(\.name), ["PIN"])
        XCTAssertEqual(items[0].customFields.first?.kind, .hidden)
        XCTAssertEqual(items[5].customFields.first?.kind, nil, "a kind from a newer app version")
        XCTAssertNil(items[1].card)
    }

    func testEditingACardAndAnIdentityKeepsOtherFields() throws {
        var item = Item(kind: .card, title: "Visa", id: "item-1", nowMs: 1)
        var card = try XCTUnwrap(item.card)
        card.number = "5555 5555 5555 4444"
        card.expiryMonth = 1
        card.expiryYear = nil
        item.card = card
        item.customFields = [CustomField(name: "PIN", value: "9876", kind: .hidden)]
        let again = try Item(json: try item.text())
        XCTAssertEqual(again.card?.lastFour, "4444")
        XCTAssertEqual(again.card?.expiryMonth, 1)
        XCTAssertNil(again.card?.expiryYear)
        XCTAssertEqual(again.customFields, [CustomField(name: "PIN", value: "9876", kind: .hidden)])

        var newer = try Item(json: try Self.fixture()[5])
        var field = try XCTUnwrap(newer.customFields.first)
        field.value = "43"
        newer.customFields = [field]
        let kept = try Item(json: try newer.text())
        XCTAssertEqual(kept.json["custom_fields"]?.array?.first?.object?["kind"], .string("otp"))
        XCTAssertEqual(kept.json["custom_fields"]?.array?.first?.object?["future_field_flag"], .bool(true))

        var identity = Identity()
        identity.addressLines = ["1 Main Street"]
        identity.city = "Athens"
        var person = Item(kind: .identity, title: "Home", id: "item-2", nowMs: 1)
        person.identity = identity
        XCTAssertEqual(try Item(json: try person.text()).identity?.city, "Athens")
    }

    func testPasswordHistoryAndPasskeys() throws {
        let items = try Self.fixture().map(Item.init(json:))
        let bank = items[8]
        XCTAssertEqual(bank.passwordHistory.map(\.password), ["second-password", "first-password"])
        XCTAssertEqual(bank.passwordHistory.map(\.replacedAtMs), [1_767_398_400_000, 1_767_312_000_000])
        XCTAssertEqual(bank.trashedAtMs, 1_767_484_800_000)
        XCTAssertEqual(items[0].passwordHistory.count, 0)
        XCTAssertEqual(items[0].login?.passkeySites, ["example.com"])
    }

    func testOneTimeCodeSetupIsCheckedByTheCore() throws {
        XCTAssertEqual(
            try OneTimeCode.normalize("JBSW Y3DP EHPK 3PXP", title: "Bank"),
            "otpauth://totp/Bank?secret=JBSWY3DPEHPK3PXP"
        )
        let uri = "otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP&issuer=Example"
        XCTAssertEqual(try OneTimeCode.normalize(uri, title: "x"), uri)
        XCTAssertThrowsError(try OneTimeCode.normalize("hello, world!", title: "x")) { error in
            XCTAssertEqual((error as? PWMError)?.code, "invalid_totp")
        }
    }

    func testNeverPrintsValues() throws {
        let item = try Item(json: try Self.fixture()[0])
        var dumped = ""
        dump(item, to: &dumped)
        let card = try Item(json: try Self.fixture()[2])
        let identity = try Item(json: try Self.fixture()[3])
        var parts = ""
        dump(card.card!, to: &parts)
        dump(identity.identity!, to: &parts)
        dump(item.customFields, to: &parts)
        for text in [
            "\(item)", String(reflecting: item), "\(item.json)", "\(item.login!)", dumped,
            "\(card.card!)", "\(identity.identity!)", "\(item.customFields)", parts,
        ] {
            XCTAssertFalse(text.contains("hunter2"), text)
            XCTAssertFalse(text.contains("alice"), text)
            XCTAssertFalse(text.contains("4111"), text)
            XCTAssertFalse(text.contains("Main Street"), text)
            XCTAssertFalse(text.contains("1234"), text)
        }
        XCTAssertThrowsError(try Item(json: "not json")) { error in
            XCTAssertEqual((error as? PWMError)?.code, "invalid_item")
        }
    }
}
