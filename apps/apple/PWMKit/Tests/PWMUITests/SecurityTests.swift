import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// The health report, devices and export models (D43 §9) over a fake account.
@MainActor
final class SecurityTests: XCTestCase {
    var account: FakeAccount!
    var pasteboard: FakePasteboard!

    override func setUp() async throws {
        account = FakeAccount()
        pasteboard = FakePasteboard()
    }

    func vault() async -> VaultModel {
        let vault = VaultModel(account: account, pasteboard: pasteboard) {}
        await vault.load()
        return vault
    }

    // MARK: Health

    func testTheReportNamesItemsAndChecksLeaksWhenOn() async throws {
        for (id, title) in [("1", "Bank"), ("2", "Shop"), ("3", "Forum"), ("4", "Mail")] {
            await account.put(loginItem(id, title, password: "x"))
        }
        await account.setHealth(
            PasswordHealth(weak: ["3": 1, "gone": 0], reused: [["2", "1"], ["4", "gone"]]),
            breached: ["4": 12]
        )
        let health = HealthModel(vault: await vault(), breachCheckOn: true)
        await health.load()
        XCTAssertEqual(health.weak.map(\.item.title), ["Forum"], "items no longer here are left out")
        XCTAssertEqual(health.reused.map { $0.map(\.item.title) }, [["Bank", "Shop"]], "a group of one is no group")
        XCTAssertEqual(health.leaked.map(\.entry.item.title), ["Mail"])
        XCTAssertEqual(health.leaked.first?.count, 12)
        XCTAssertTrue(health.checkedLeaks)
        XCTAssertFalse(health.isClean)
        XCTAssertEqual(health.edit(health.weak[0]).title, "Forum")
    }

    func testTheLeakCheckIsSkippedWhenOffAndItsFailureIsShown() async throws {
        await account.put(loginItem("1", "Bank", password: "x"))
        await account.setHealth(PasswordHealth(weak: [:], reused: []), breached: ["1": 3])
        let off = HealthModel(vault: await vault(), breachCheckOn: false)
        await off.load()
        XCTAssertTrue(off.isClean)
        XCTAssertFalse(off.checkedLeaks)
        let calls = await account.calls
        XCTAssertFalse(calls.contains("breaches"), "nothing left the device")

        await account.setHealth(PasswordHealth(weak: [:], reused: []), breachFailure: "network")
        let failing = HealthModel(vault: await vault(), breachCheckOn: true)
        await failing.load()
        XCTAssertTrue(failing.loaded, "the local checks still show")
        XCTAssertEqual(failing.error, PWMError(code: "network").message)
    }

    func testTheSwitchIsKeptAndOnByDefault() async throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "pwm-health-\(UUID().uuidString)"))
        let app = AppModel(makeAccount: { [unowned self] _ in self.account }, defaults: defaults, pasteboard: pasteboard, deviceName: "iPhone")
        XCTAssertTrue(app.breachCheckOn)
        app.breachCheckOn = false
        XCTAssertFalse(AppModel(makeAccount: { [unowned self] _ in self.account }, defaults: defaults, pasteboard: pasteboard, deviceName: "iPhone").breachCheckOn)
    }

    // MARK: Devices

    func device(_ id: String, _ name: String?, current: Bool = false, revoked: Bool = false) -> DeviceEntry {
        DeviceEntry(
            id: id, platform: "ios", name: name, added: Date(timeIntervalSince1970: 1),
            lastSeen: Date(timeIntervalSince1970: 2), revoked: revoked ? Date() : nil, isCurrent: current
        )
    }

    func testRenameAndRevoke() async throws {
        await account.setDevices([device("a", "iPhone", current: true), device("b", nil), device("c", "Old", revoked: true)])
        let devices = DevicesModel(account: account)
        await devices.load()
        XCTAssertEqual(devices.devices.map(\.name), ["iPhone", nil, "Old"])

        var renamed = await devices.rename(devices.devices[1], to: "  Work Mac ")
        XCTAssertTrue(renamed)
        XCTAssertEqual(devices.devices[1].name, "Work Mac")
        renamed = await devices.rename(devices.devices[0], to: "   ")
        XCTAssertFalse(renamed, "an empty name is not sent")
        renamed = await devices.rename(devices.devices[2], to: "New")
        XCTAssertFalse(renamed, "a revoked device keeps its name")

        await devices.revoke(devices.devices[0])
        await devices.revoke(devices.devices[1])
        let calls = await account.calls
        XCTAssertEqual(calls, ["rename b Work Mac", "revoke b"], "this device is never revoked")
        XCTAssertNotNil(devices.devices[1].revoked)

        await account.fail(with: "network")
        await devices.load()
        XCTAssertEqual(devices.error, PWMError(code: "network").message)
    }

    // MARK: Export

    func testAnEncryptedExportNeedsTheMasterPasswordAndAStrongExportPassword() async throws {
        let export = ExportModel(account: account, today: { Date(timeIntervalSince1970: 1_767_268_800) })
        XCTAssertEqual(export.choice, .encrypted)
        export.masterPassword = strong
        export.exportPassword = "password"
        await export.updateStrength()
        export.confirm = "password"
        XCTAssertFalse(export.canExport)

        export.exportPassword = "granite-sparrow-lattice-91"
        await export.updateStrength()
        XCTAssertFalse(export.canExport, "the confirmation differs")
        export.confirm = "granite-sparrow-lattice-91"
        XCTAssertTrue(export.canExport)

        export.masterPassword = "wrong"
        await export.export()
        XCTAssertEqual(export.error, "Wrong master password.")
        XCTAssertEqual(export.masterPassword, "")
        XCTAssertNil(export.file)

        export.masterPassword = strong
        await export.export()
        XCTAssertEqual(export.file?.value, Data("encrypted".utf8))
        XCTAssertTrue(export.fileName.hasPrefix("PWM export 2026-01-01"), export.fileName)
        XCTAssertTrue(export.fileName.hasSuffix(".pwmx"))
        XCTAssertEqual(export.masterPassword, "")
        XCTAssertEqual(export.exportPassword, "")
        export.done()
        XCTAssertNil(export.file)
        let calls = await account.calls
        XCTAssertEqual(calls, ["export pwmx granite-sparrow-lattice-91"])
    }

    func testAPlaintextExportNeedsTheWarningRead() async throws {
        let export = ExportModel(account: account)
        export.choice = .csv
        export.masterPassword = strong
        XCTAssertFalse(export.canExport)
        export.understood = true
        XCTAssertTrue(export.canExport)
        await export.export()
        XCTAssertEqual(export.file?.value, Data("title".utf8))
        XCTAssertTrue(export.fileName.hasSuffix(".csv"))
        export.done()
        XCTAssertFalse(export.understood, "asked again next time")
    }
}
