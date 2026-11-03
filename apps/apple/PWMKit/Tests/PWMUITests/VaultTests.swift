import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// The vault screens' models (D43 §9) over a fake account.
@MainActor
final class VaultTests: XCTestCase {
    var account: FakeAccount!
    var pasteboard: FakePasteboard!
    var signedOut = 0

    override func setUp() async throws {
        account = FakeAccount()
        pasteboard = FakePasteboard()
        signedOut = 0
    }

    func vault() async -> VaultModel {
        let vault = VaultModel(account: account, pasteboard: pasteboard) { [unowned self] in self.signedOut += 1 }
        await vault.load()
        return vault
    }

    func trashed(_ item: Item) throws -> Item {
        var json = item.json
        json["trashed_at"] = .int(5)
        return try Item(json: try JSONValue.object(json).text())
    }

    // MARK: Search and sections

    func testSearchMatchesTitleUsernameURLAndLastFour() async throws {
        await account.put(loginItem("1", "Example", username: "ada@example.com", url: "https://example.com"))
        await account.put(loginItem("2", "Bank", username: "lovelace", url: "https://bank.example"))
        var card = Item(kind: .card, title: "Visa", id: "3", nowMs: 1)
        var fields = Card()
        fields.number = "4111 1111 1111 4242"
        card.card = fields
        await account.put(card)
        let vault = await vault()

        func found(_ query: String) -> [String] {
            vault.query = query
            return vault.visible.map(\.id)
        }
        XCTAssertEqual(found(""), ["2", "1", "3"], "by title")
        XCTAssertEqual(found("  EXAMPLE "), ["2", "1"], "trimmed, any case, title or URL")
        XCTAssertEqual(found("lovel"), ["2"])
        XCTAssertEqual(found("bank.example"), ["2"])
        XCTAssertEqual(found("4242"), ["3"])
        XCTAssertEqual(found("4111"), [], "only the last 4 digits")
        XCTAssertEqual(found("hunter2"), [], "never passwords")
    }

    func testOrderIsByTitleThenId() {
        let entries = [("b", "Mail"), ("a", "Mail"), ("c", "Apple"), ("d", "item 10"), ("e", "item 9")].map {
            Entry(vaultId: "v", item: loginItem($0.0, $0.1))
        }
        let ids = VaultModel.filter(entries, section: .all, query: "").map(\.id)
        XCTAssertEqual(ids, ["c", "e", "d", "a", "b"], "numbers in titles order as numbers; the same title by id")
    }

    func testSectionsAndTrash() async throws {
        var favorite = loginItem("1", "Favorite")
        favorite.favorite = true
        favorite.folderId = "f"
        var folder = Item(kind: .folder, title: "Work", id: "f", nowMs: 1)
        folder.folderId = nil
        await account.put(favorite)
        await account.put(folder)
        await account.put(Item(kind: .note, title: "Note", id: "2", nowMs: 1))
        await account.put(try trashed(loginItem("3", "Old")))
        let vault = await vault()

        func shown(_ section: VaultModel.Section) -> [String] {
            vault.section = section
            return vault.visible.map(\.id)
        }
        XCTAssertEqual(shown(.all), ["1", "2"], "no folders, no trash")
        XCTAssertEqual(shown(.favorites), ["1"])
        XCTAssertEqual(shown(.kind(.note)), ["2"])
        XCTAssertEqual(shown(.kind(.card)), [])
        XCTAssertEqual(shown(.folder("f")), ["1"])
        XCTAssertEqual(shown(.trash), ["3"])
        XCTAssertEqual(vault.folders.map(\.item.title), ["Work"])

        vault.query = "old"
        XCTAssertEqual(vault.visible.map(\.id), ["3"], "search within the trash")
        vault.section = .all
        XCTAssertEqual(vault.visible.map(\.id), [], "trashed items are not found elsewhere")
    }

    func testTrashRestoreAndDeleteForever() async throws {
        await account.put(loginItem("1", "Example"))
        let vault = await vault()
        let entry = try XCTUnwrap(vault.entry("1"))
        await vault.trash(entry)
        XCTAssertEqual(vault.visible.count, 0)
        vault.section = .trash
        XCTAssertEqual(vault.visible.map(\.id), ["1"])
        await vault.restore(try XCTUnwrap(vault.entry("1")))
        XCTAssertEqual(vault.visible.count, 0)
        vault.section = .all
        XCTAssertEqual(vault.visible.map(\.id), ["1"])
        await vault.deleteForever(try XCTUnwrap(vault.entry("1")))
        XCTAssertNil(vault.entry("1"))
        let calls = await account.calls
        XCTAssertEqual(calls.filter { ["trash", "restore", "delete"].contains($0) }, ["trash", "restore", "delete"])
    }

    func testFiveThousandItemsFilterQuickly() async throws {
        let entries = (0..<5_000).map { n in
            Entry(vaultId: "v", item: loginItem("id-\(n)", "Site \(n)", username: "user\(n)@example.com", url: "https://site\(n).example"))
        }
        let started = ContinuousClock.now
        let all = VaultModel.filter(entries, section: .all, query: "")
        let some = VaultModel.filter(entries, section: .all, query: "site49")
        let took = ContinuousClock.now - started
        XCTAssertEqual(all.count, 5_000)
        XCTAssertEqual(some.count, 111, "site49, site490–499, site4900–4999")
        XCTAssertLessThan(took, .seconds(2), "\(took)")
    }

    // MARK: Sync

    func testRefreshSyncsPurgesAndReportsProblems() async throws {
        let vault = await vault()
        await vault.refresh()
        XCTAssertNil(vault.status)
        var calls = await account.calls
        XCTAssertEqual(calls, ["sync", "purge"])

        await account.setSyncResult(.retry(afterMs: 1000))
        await vault.refresh()
        XCTAssertEqual(vault.status, "Couldn't sync. Showing what's on this device.")

        await account.setSyncResult(.synced(pulled: 0, pushed: 0, conflicts: 0, failed: 2))
        await vault.refresh()
        XCTAssertEqual(vault.status, "Some changes couldn't sync. They'll be sent again.")

        await account.fail(with: "network")
        await vault.refresh()
        XCTAssertEqual(vault.status, PWMError(code: "network").message)

        await account.fail(with: nil)
        await account.setSyncResult(.deviceRevoked)
        await vault.refresh()
        XCTAssertEqual(signedOut, 1)
        calls = await account.calls
        XCTAssertEqual(calls.last, "signOut")
    }

    // MARK: Detail

    func testDetailMasksCopiesAndShowsTheCode() async throws {
        var item = loginItem("1", "Example", username: "ada", password: "hunter2")
        item.login?.totp = "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP"
        await account.put(item)
        await account.setCode("123456", secondsLeft: 12)
        let vault = await vault()
        let detail = vault.detail(try XCTUnwrap(vault.entry("1")))

        XCTAssertFalse(detail.isRevealed("password"))
        detail.toggle("password")
        XCTAssertTrue(detail.isRevealed("password"))
        detail.copy(detail.item.login!.password, field: "password")
        XCTAssertEqual(pasteboard.copied.map(\.0), ["hunter2"])
        XCTAssertEqual(pasteboard.copied.map(\.1), [30])
        XCTAssertEqual(detail.copied, "password")

        await detail.refreshCode()
        XCTAssertEqual(detail.code?.value.value, "123456")
        XCTAssertEqual(detail.code?.secondsLeft, 12)
        detail.copyCode()
        XCTAssertEqual(pasteboard.copied.last?.0, "123456")

        detail.hideAll()
        XCTAssertFalse(detail.isRevealed("password"))
        XCTAssertNil(detail.code)

        await account.setCode(nil, fails: true)
        await detail.refreshCode()
        XCTAssertTrue(detail.codeUnsupported)
        XCTAssertEqual(ItemDetailModel.masked("hunter2"), "••••••••")
    }

    // MARK: Editing

    func testANewLoginWithAGeneratedPasswordAndACode() async throws {
        let vault = await vault()
        let editMade = await vault.newItem(.login)
        let edit = try XCTUnwrap(editMade)
        XCTAssertTrue(edit.isNew)
        XCTAssertFalse(edit.canSave, "needs a title")
        XCTAssertFalse(edit.hasChanges, "an untouched new item closes without asking")
        edit.title = "  Bank  "
        edit.login.username = "ada"
        edit.addURL()
        edit.urls[0].value.url = "https://bank.example"
        edit.addURL()
        edit.use(Redacted("generated-password"))
        edit.totp = "JBSW Y3DP EHPK 3PXP"
        edit.tags = "money, , home "
        XCTAssertTrue(edit.canSave)
        let saved = await edit.save()
        XCTAssertTrue(saved)

        let item = try XCTUnwrap(vault.entries.first?.item)
        XCTAssertEqual(item.title, "Bank")
        XCTAssertEqual(item.login?.password, "generated-password")
        XCTAssertEqual(item.login?.urls.map(\.url), ["https://bank.example"], "the empty URL is dropped")
        XCTAssertEqual(item.login?.totp, "otpauth://totp/Bank?secret=JBSWY3DPEHPK3PXP", "checked by the core")
        XCTAssertEqual(item.tags, ["money", "home"])
    }

    func testABadCodeIsRefusedAndNothingIsSaved() async throws {
        let vault = await vault()
        let editMade = await vault.newItem(.login)
        let edit = try XCTUnwrap(editMade)
        edit.title = "Bank"
        edit.totp = "not a code!"
        let saved = await edit.save()
        XCTAssertFalse(saved)
        XCTAssertEqual(edit.error, PWMError(code: "invalid_totp").message)
        XCTAssertTrue(vault.entries.isEmpty)
    }

    func testOpeningAnItemIsNotAChangeAndEditsKeepUnknownFields() async throws {
        let fixture = try ItemFixture.items()
        await account.put(fixture[5])
        await account.put(fixture[0])
        let vault = await vault()

        let untouched = vault.edit(try XCTUnwrap(vault.entry(fixture[0].id)))
        XCTAssertFalse(untouched.hasChanges, "no discard prompt")
        XCTAssertFalse(untouched.canSave)

        let edit = vault.edit(try XCTUnwrap(vault.entry(fixture[5].id)))
        XCTAssertFalse(edit.hasChanges)
        edit.login.password = "changed"
        XCTAssertTrue(edit.hasChanges, "asks to discard")
        await edit.save()
        let savedMade = await account.storedItem(fixture[5].id!)
        let saved = try XCTUnwrap(savedMade)
        XCTAssertEqual(saved.login?.password, "changed")
        XCTAssertEqual(saved.json["future_top_level"], fixture[5].json["future_top_level"])
        XCTAssertEqual(saved.login?.json["future_login_field"], .string("keep me"))
        XCTAssertEqual(saved.login?.urls.first?.match, "fuzzy")
        XCTAssertEqual(saved.json["custom_fields"], fixture[5].json["custom_fields"])
    }

    func testCardsIdentitiesAndCustomFields() async throws {
        let vault = await vault()
        let cardMade = await vault.newItem(.card)
        let card = try XCTUnwrap(cardMade)
        card.title = "Visa"
        card.card.number = "4111 1111 1111 1111"
        card.expiryMonth = "13"
        card.expiryYear = "2030"
        var saved = await card.save()
        XCTAssertFalse(saved)
        XCTAssertEqual(card.error, PWMError(code: "invalid_expiry_month").message)
        card.expiryMonth = "7"
        card.addField(.hidden)
        card.customFields[0].value.name = "PIN"
        card.customFields[0].value.value = "9876"
        card.addField(.text)
        saved = await card.save()
        XCTAssertTrue(saved)
        let stored = try XCTUnwrap(vault.entries.first { $0.item.kind == .card }?.item)
        XCTAssertEqual(stored.card?.expiryMonth, 7)
        XCTAssertEqual(stored.card?.expiryYear, 2030)
        XCTAssertEqual(stored.customFields, [CustomField(name: "PIN", value: "9876", kind: .hidden)], "the empty field is dropped")

        let identityMade = await vault.newItem(.identity)
        let identity = try XCTUnwrap(identityMade)
        identity.title = "Home"
        identity.identity.city = "Athens"
        identity.address = "1 Main Street\n\n Flat 2 "
        saved = await identity.save()
        XCTAssertTrue(saved)
        let person = try XCTUnwrap(vault.entries.first { $0.item.kind == .identity }?.item)
        XCTAssertEqual(person.identity?.addressLines, ["1 Main Street", "Flat 2"])
        XCTAssertEqual(person.identity?.city, "Athens")
    }

    func testNewerItemsAreReadOnly() async throws {
        let fixture = try ItemFixture.items()
        await account.put(fixture[7])
        let vault = await vault()
        let edit = vault.edit(try XCTUnwrap(vault.entry(fixture[7].id)))
        XCTAssertFalse(edit.isEditable)
        edit.title = "Changed"
        XCTAssertFalse(edit.canSave)
        let saved = await edit.save()
        XCTAssertFalse(saved)
    }

    func testNewItemsGoToTheShownFolderAndANewFolderIsMadeAtOnce() async throws {
        await account.put(Item(kind: .folder, title: "Work", id: "f", nowMs: 1))
        let vault = await vault()
        vault.section = .folder("f")
        let editMade = await vault.newItem(.note)
        let edit = try XCTUnwrap(editMade)
        XCTAssertEqual(edit.folderId, "f")

        await edit.newFolder(named: "  Home ")
        XCTAssertEqual(edit.folders.map(\.item.title), ["Home", "Work"])
        let home = try XCTUnwrap(edit.folders.first)
        XCTAssertEqual(edit.folderId, home.id)
        edit.title = "Wi-Fi"
        await edit.save()
        XCTAssertEqual(vault.entries.first { $0.item.title == "Wi-Fi" }?.item.folderId, home.id)
    }

    // MARK: Generator

    func testGeneratorOptions() async throws {
        let generator = GeneratorModel(pasteboard: pasteboard)
        await generator.regenerate()
        XCTAssertEqual(generator.value?.value.count, 20)
        XCTAssertGreaterThanOrEqual(generator.strength?.score ?? 0, 3)

        generator.length = 12
        generator.symbols = false
        generator.uppercase = false
        await generator.regenerate()
        let value = try XCTUnwrap(generator.value?.value)
        XCTAssertEqual(value.count, 12)
        XCTAssertTrue(value.allSatisfy { $0.isLowercase || $0.isNumber }, "only lowercase and digits")

        generator.lowercase = false
        generator.digits = false
        await generator.regenerate()
        XCTAssertNil(generator.value)
        XCTAssertEqual(generator.error, "Choose at least one kind of character.")

        generator.mode = .passphrase
        generator.words = 4
        generator.separator = "."
        await generator.regenerate()
        XCTAssertEqual(generator.value?.value.split(separator: ".").count, 4)
        XCTAssertNil(generator.error)
        generator.copy()
        XCTAssertTrue(generator.copied)
        XCTAssertEqual(pasteboard.copied.last?.1, 30)
    }
}

/// The lock timeout with a clock the test moves (D43 §5).
@MainActor
final class LockTimeoutTests: XCTestCase {
    var account: FakeAccount!
    var defaults: UserDefaults!
    var now = ContinuousClock.now

    override func setUp() async throws {
        account = FakeAccount()
        defaults = UserDefaults(suiteName: "pwm-lock-\(UUID().uuidString)")
        now = ContinuousClock.now
        await account.setSignedIn(email: "ada@example.com")
    }

    func unlocked() async throws -> AppModel {
        let app = AppModel(
            makeAccount: { [unowned self] _ in self.account },
            defaults: defaults,
            pasteboard: FakePasteboard(),
            deviceName: "iPhone",
            clock: { [unowned self] in self.now }
        )
        await app.start()
        let unlock = try XCTUnwrap(app.unlock)
        unlock.password = strong
        await unlock.submit()
        XCTAssertEqual(app.screen, .vault)
        return app
    }

    func testTheDefaultIsFifteenMinutesInTheBackground() async throws {
        let app = try await unlocked()
        XCTAssertEqual(app.lockTimeout, .fifteenMinutes)
        await app.didEnterBackground()
        now += .seconds(14 * 60)
        await app.willEnterForeground()
        XCTAssertEqual(app.screen, .vault)

        await app.didEnterBackground()
        now += .seconds(15 * 60)
        await app.willEnterForeground()
        XCTAssertEqual(app.screen, .unlock)
    }

    func testImmediatelyLocksOnLeaving() async throws {
        let app = try await unlocked()
        app.lockTimeout = .immediately
        XCTAssertEqual(LockTimeout.saved(in: defaults), .immediately, "kept")
        await app.didEnterBackground()
        XCTAssertEqual(app.screen, .unlock)
    }

    func testTimeInTheForegroundDoesNotCount() async throws {
        let app = try await unlocked()
        app.lockTimeout = .oneMinute
        now += .seconds(3600)
        await app.willEnterForeground()
        XCTAssertEqual(app.screen, .vault)
    }

    func testLockingTheDeviceLocksTheVault() async throws {
        let app = try await unlocked()
        app.lockTimeout = .eightHours
        await app.deviceWillLock()
        XCTAssertEqual(app.screen, .unlock)
        let calls = await account.calls
        XCTAssertEqual(calls.suffix(1), ["lock"])
    }

    func testAnUnknownSavedValueFallsBackToTheDefault() {
        defaults.set(7, forKey: "lockTimeoutMinutes")
        XCTAssertEqual(LockTimeout.saved(in: defaults), .fifteenMinutes)
    }
}

/// The core's D18 fixture, as `Item`s.
enum ItemFixture {
    static func items() throws -> [Item] {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let data = try Data(contentsOf: root.appendingPathComponent("core/pwm-vault/tests/fixtures/items_v1.json"))
        let values = try JSONDecoder().decode([JSONValue].self, from: data)
        return try values.enumerated().map { index, value in
            var object = value.object ?? [:]
            object["id"] = .string("fixture-\(index)")
            return try Item(json: try JSONValue.object(object).text())
        }
    }
}
