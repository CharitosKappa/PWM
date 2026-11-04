import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// The import screen's model (D51 §9) over a fake account and fake files.
@MainActor
final class ImportTests: XCTestCase {
    var account: FakeAccount!
    var files: FakeImportFiles!

    override func setUp() async throws {
        account = FakeAccount()
        files = FakeImportFiles()
    }

    func model() -> ImportModel {
        ImportModel(account: account, files: files)
    }

    func file(_ name: String, _ text: String) -> URL {
        let url = URL(fileURLWithPath: "/picked/\(name)")
        files.files[url] = Data(text.utf8)
        return url
    }

    let lastpass = "url,username,password,totp,extra,name,grouping,fav\nhttps://b.example,ada,pw,,,Bank,,0\n"

    func testAFileIsCheckedImportedAndDeletedOnRequest() async {
        let model = model()
        let url = file("lastpass.csv", lastpass)
        await model.picked(url)
        XCTAssertEqual(model.fileName, "lastpass.csv")
        XCTAssertEqual(model.format, "lastpass")
        XCTAssertFalse(model.asksPassword)
        XCTAssertTrue(model.canCheck)

        await model.check()
        let summary = try? XCTUnwrap(model.summary)
        XCTAssertEqual(summary.map(ImportModel.counts), "2 logins, 1 folder")
        XCTAssertEqual(summary?.leftOut.map(ImportModel.leftOut), ["Left out: 1 attachments and documents"])

        await model.importNow()
        XCTAssertNotNil(model.done)
        XCTAssertNil(model.summary)
        XCTAssertTrue(model.plaintext, "no password: plain text")
        XCTAssertTrue(model.canDelete)
        let calls = await account.calls
        XCTAssertEqual(calls, ["prepareImport lastpass", "finishImport"])

        model.deleteFile()
        XCTAssertTrue(model.deleted)
        XCTAssertEqual(files.deleted, [url])
        XCTAssertFalse(model.canDelete)
    }

    func testAnEncryptedExportAsksForItsPassword() async {
        let model = model()
        await model.picked(file("bitwarden.json", "{\"encrypted\":true}"))
        XCTAssertEqual(model.format, "bitwarden")
        XCTAssertTrue(model.asksPassword)

        await model.check()
        XCTAssertEqual(model.error, "Enter the password this export was made with.")
        model.password = "nope"
        await model.check()
        XCTAssertEqual(model.error, "Wrong password for this export.")
        model.password = "export-pw"
        await model.check()
        XCTAssertNil(model.error)
        await model.importNow()
        XCTAssertFalse(model.plaintext, "an export with a password isn't plain text")
        XCTAssertFalse(model.canDelete)
        XCTAssertEqual(model.password, "", "the password is dropped")
    }

    func testAGenericCSVGetsAColumnPicker() async {
        let model = model()
        await model.picked(file("passwords.csv", "url,name,username,password\nhttps://b.example,Bank,ada,pw\n"))
        XCTAssertEqual(model.format, "csv")
        XCTAssertEqual(model.columns?.headers, ["url", "name", "username", "password"])
        XCTAssertEqual(model.columnMap.title, 1)
        model.columnMap.title = 0
        await model.check()
        let calls = await account.calls
        XCTAssertEqual(calls, ["prepareImport csv title=0"])
        // Changing a column drops the preview, so it is checked again.
        model.columnMap.title = 1
        XCTAssertNil(model.summary)
    }

    func testAnUnknownFileNeedsAFormatChosen() async {
        let model = model()
        await model.picked(file("photo.png", "\u{89}PNG"))
        XCTAssertEqual(model.format, "")
        XCTAssertFalse(model.canCheck)
        model.format = "keeper"
        await model.check()
        XCTAssertEqual(model.error, "This file can't be read as that format. Check the format, or export again.")
    }

    func testAFileTooLargeIsRefusedBeforeReading() async {
        let model = model()
        let url = URL(fileURLWithPath: "/picked/huge.csv")
        files.files[url] = Data(count: ImportFile.maxBytes + 1)
        await model.picked(url)
        XCTAssertEqual(model.error, "This file is too large to import.")
        XCTAssertNil(model.fileName)
    }

    func testAKeePassDatabaseTakesAKeyFile() async {
        let model = model()
        await model.picked(file("vault.kdbx", "kdbx"))
        model.format = "keepass"
        XCTAssertTrue(model.asksPassword)
        XCTAssertTrue(model.asksKeyFile)
        await model.check()
        XCTAssertEqual(model.error, "Enter the password this export was made with.")

        let key = file("vault.keyx", "key")
        model.pickedKeyFile(key)
        XCTAssertEqual(model.keyFileName, "vault.keyx")
        await model.check()
        XCTAssertNil(model.error)
        await model.importNow()
        XCTAssertFalse(model.plaintext, "a database with a key file isn't plain text")
        XCTAssertNil(model.keyFileName, "the key file is dropped")
        let calls = await account.calls
        XCTAssertEqual(calls, ["prepareImport keepass key", "finishImport"])
    }

    func testLeavingDropsThePreview() async {
        let model = model()
        await model.picked(file("lastpass.csv", lastpass))
        await model.check()
        await model.close()
        XCTAssertNil(model.summary)
        let calls = await account.calls
        XCTAssertEqual(calls.last, "cancelImport")
    }
}
