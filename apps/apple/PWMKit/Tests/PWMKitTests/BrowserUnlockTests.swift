import Foundation
import XCTest
@testable import PWMKit

/// The browser helper (D48 §5): enrol, unlock, forget, who may ask, Touch
/// ID cancelled or changed, the framing and the browsers' manifests.
final class BrowserUnlockTests: XCTestCase {
    let account = String(repeating: "a1", count: 16)
    let device = String(repeating: "d1", count: 16)
    let chrome = BrowserUnlock.chromiumOrigins[0]
    let key = Data(repeating: 7, count: 32)

    var store: MemoryBrowserKeyStore!
    var box: Box!
    var helper: BrowserUnlockHelper!

    override func setUp() {
        store = MemoryBrowserKeyStore()
        let box = Box()
        helper = BrowserUnlockHelper(store: store) { id in box.biometrics(id) }
        self.box = box
    }

    /// One fake Secure Enclave key per enrolment, as the real helper has.
    final class Box: @unchecked Sendable {
        private var keys: [String: FakeBiometrics] = [:]
        private let lock = NSLock()

        func biometrics(_ id: String) -> FakeBiometrics {
            lock.lock()
            defer { lock.unlock() }
            if let found = keys[id] { return found }
            let made = FakeBiometrics()
            keys[id] = made
            return made
        }
    }

    func send(_ type: String, key: Data? = nil, caller: String? = nil, account: String? = nil) async -> BrowserUnlock.Reply {
        let request = BrowserUnlock.Request(
            type: type, accountId: account ?? self.account, deviceId: device, key: key?.base64EncodedString()
        )
        let data = try! JSONEncoder().encode(request)
        let reply = await helper.handle(data, caller: caller ?? chrome)
        return try! JSONDecoder().decode(BrowserUnlock.Reply.self, from: reply)
    }

    func enrolled() -> FakeBiometrics {
        box.biometrics(BrowserUnlockHelper.enrolment(caller: chrome, account: account, device: device))
    }

    func testEnrolThenEachUnlockAsksForTouchID() async {
        let enrol = await send("enroll", key: key)
        XCTAssertEqual(enrol, .done)
        XCTAssertEqual(enrolled().prompts, [BrowserUnlockHelper.enrollReason], "turning it on asks once")
        XCTAssertEqual(store.count, 1)
        let kept = store.get(BrowserUnlockHelper.enrolment(caller: chrome, account: account, device: device))
        XCTAssertNotNil(kept)
        XCTAssertNotEqual(kept, key.base64EncodedString(), "only the wrapped key is kept")

        let unlock = await send("unlock")
        XCTAssertTrue(unlock.ok)
        XCTAssertEqual(unlock.key, key.base64EncodedString())
        XCTAssertEqual(enrolled().prompts.last, BrowserUnlockHelper.unlockReason)

        let forget = await send("forget")
        XCTAssertEqual(forget, .done)
        XCTAssertEqual(store.count, 0)
        XCTAssertFalse(enrolled().hasKey)
        let after = await send("unlock")
        XCTAssertEqual(after, .failed("unknown"))
    }

    func testOnlyOurExtensionsMayAsk() async {
        for caller in ["chrome-extension://someoneelse/", "evil@example.com", ""] {
            let reply = await send("enroll", key: key, caller: caller)
            XCTAssertEqual(reply, .failed("refused"), caller)
        }
        let none = await helper.handle(Data("{}".utf8), caller: nil)
        XCTAssertEqual(try JSONDecoder().decode(BrowserUnlock.Reply.self, from: none), .failed("refused"))
        XCTAssertEqual(store.count, 0)
        // Firefox and Safari callers are our own too.
        for caller in BrowserUnlock.firefoxIds + [BrowserUnlock.safariCaller] {
            let reply = await send("enroll", key: key, caller: caller)
            XCTAssertEqual(reply, .done, caller)
        }
    }

    func testEnrolmentsAreKeptApart() async {
        _ = await send("enroll", key: key)
        let other = String(repeating: "a2", count: 16)
        let elsewhere = await send("unlock", account: other)
        XCTAssertEqual(elsewhere, .failed("unknown"))
        let firefox = await send("unlock", caller: BrowserUnlock.firefoxIds[0])
        XCTAssertEqual(firefox, .failed("unknown"), "another browser is another enrolment")
    }

    func testCancelledAndChangedTouchID() async {
        _ = await send("enroll", key: key)
        enrolled().cancel = true
        let cancelled = await send("unlock")
        XCTAssertEqual(cancelled, .failed("cancelled"))
        XCTAssertEqual(store.count, 1, "a cancel keeps it")
        enrolled().cancel = false

        enrolled().changed = true
        let changed = await send("unlock")
        XCTAssertEqual(changed, .failed("changed"))
        XCTAssertEqual(store.count, 0, "a changed face or finger deletes it")
    }

    func testCancellingTheFirstTouchIDEnrolsNothing() async {
        let id = BrowserUnlockHelper.enrolment(caller: chrome, account: account, device: device)
        box.biometrics(id).cancel = true
        let reply = await send("enroll", key: key)
        XCTAssertEqual(reply, .failed("cancelled"))
        XCTAssertEqual(store.count, 0)
        XCTAssertFalse(box.biometrics(id).hasKey)
    }

    func testRefusesWhatIsNotWellFormed() async {
        let noHardware = enrolled()
        noHardware.available = .unavailable(reason: "none")
        let unavailable = await send("enroll", key: key)
        XCTAssertEqual(unavailable, .failed("unavailable"))
        noHardware.available = .available(.touchID)

        let short = await send("enroll", key: Data(repeating: 1, count: 31))
        XCTAssertEqual(short, .failed("invalid"))
        let badId = await send("unlock", account: "not-an-id")
        XCTAssertEqual(badId, .failed("invalid"))
        let unknownType = await send("export")
        XCTAssertEqual(unknownType, .failed("invalid"))
        var newer = BrowserUnlock.Request(type: "unlock", accountId: account, deviceId: device)
        newer.v = 2
        let reply = await helper.handle(try! JSONEncoder().encode(newer), caller: chrome)
        XCTAssertEqual(try JSONDecoder().decode(BrowserUnlock.Reply.self, from: reply), .failed("invalid"))
        let garbage = await helper.handle(Data([0xFF, 0x00]), caller: chrome)
        XCTAssertEqual(try JSONDecoder().decode(BrowserUnlock.Reply.self, from: garbage), .failed("invalid"))
    }

    func testTheCallerComesFromTheArguments() {
        XCTAssertEqual(BrowserUnlock.caller(arguments: ["helper", chrome, "--parent-window=0"]), chrome)
        XCTAssertEqual(BrowserUnlock.caller(arguments: ["helper", "/path/manifest.json", "pwm@pwm.invalid"]), "pwm@pwm.invalid")
        XCTAssertNil(BrowserUnlock.caller(arguments: ["helper"]))
    }

    func testFraming() throws {
        let message = Data(#"{"v":1}"#.utf8)
        let framed = NativeMessaging.frame(message)
        XCTAssertEqual([UInt8](framed.prefix(4)), [7, 0, 0, 0], "little-endian length")
        let parsed = try XCTUnwrap(NativeMessaging.unframe(framed + Data([1, 2])))
        XCTAssertEqual(parsed.message, message)
        XCTAssertEqual(parsed.rest, Data([1, 2]))
        XCTAssertNil(try NativeMessaging.unframe(framed.prefix(6)), "waits for the rest")
        XCTAssertNil(try NativeMessaging.unframe(Data([1, 0])))
        var huge = UInt32(NativeMessaging.maxIncoming + 1).littleEndian
        XCTAssertThrowsError(try NativeMessaging.unframe(Data(bytes: &huge, count: 4)))
    }

    func testManifestsGoOnlyToInstalledBrowsers() throws {
        let home = FileManager.default.temporaryDirectory.appendingPathComponent("pwm-home-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: home) }
        let support = home.appendingPathComponent("Library/Application Support")
        for folder in ["Google/Chrome", "Mozilla"] {
            try FileManager.default.createDirectory(at: support.appendingPathComponent(folder), withIntermediateDirectories: true)
        }
        let hosts = BrowserHosts(home: home, helperPath: "/Applications/PWM.app/Contents/Library/LoginItems/PWM Browser Helper.app/Contents/MacOS/PWM Browser Helper")
        XCTAssertFalse(hosts.isInstalled)
        XCTAssertEqual(try hosts.install(), ["Google/Chrome", "Mozilla"])
        XCTAssertTrue(hosts.isInstalled)
        XCTAssertFalse(FileManager.default.fileExists(atPath: support.appendingPathComponent("Microsoft Edge").path), "nothing made for a browser that isn't there")

        let chrome = try JSONSerialization.jsonObject(with: Data(contentsOf: support.appendingPathComponent("Google/Chrome/NativeMessagingHosts/\(BrowserUnlock.hostName).json"))) as? [String: Any]
        XCTAssertEqual(chrome?["name"] as? String, BrowserUnlock.hostName)
        XCTAssertEqual(chrome?["type"] as? String, "stdio")
        XCTAssertEqual(chrome?["path"] as? String, hosts.helperPath)
        XCTAssertEqual(chrome?["allowed_origins"] as? [String], BrowserUnlock.chromiumOrigins)
        XCTAssertNil(chrome?["allowed_extensions"])
        let firefox = try JSONSerialization.jsonObject(with: Data(contentsOf: support.appendingPathComponent("Mozilla/NativeMessagingHosts/\(BrowserUnlock.hostName).json"))) as? [String: Any]
        XCTAssertEqual(firefox?["allowed_extensions"] as? [String], BrowserUnlock.firefoxIds)
        XCTAssertNil(firefox?["allowed_origins"])

        hosts.remove()
        XCTAssertFalse(hosts.isInstalled)
    }
}
