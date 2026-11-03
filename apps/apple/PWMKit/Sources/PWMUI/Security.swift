import Foundation
import Observation
import PWMKit

// The health report, the device list and export (D43 §6–8), reached from
// Settings.

/// Weak, reused and leaked passwords (D22). The leak check runs when it is
/// switched on in Settings; nothing is stored.
@MainActor
@Observable
public final class HealthModel {
    public struct Leaked: Sendable, Equatable, Identifiable {
        public let entry: Entry
        /// How many times the password was seen in breaches.
        public let count: UInt64
        public var id: String { entry.id }
    }

    public private(set) var weak: [Entry] = []
    public private(set) var reused: [[Entry]] = []
    public private(set) var leaked: [Leaked] = []
    public private(set) var loaded = false
    public private(set) var checkingLeaks = false
    /// The leak check ran in this report.
    public private(set) var checkedLeaks = false
    public private(set) var error: String?

    let vault: VaultModel
    private let breachCheckOn: Bool

    init(vault: VaultModel, breachCheckOn: Bool) {
        self.vault = vault
        self.breachCheckOn = breachCheckOn
    }

    public var isClean: Bool { loaded && weak.isEmpty && reused.isEmpty && leaked.isEmpty }

    /// The local checks first, then the leak check if it is on.
    public func load() async {
        await vault.load()
        let byId = Dictionary(vault.entries.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        do {
            let health = try await vault.account.passwordHealth()
            weak = health.weak.keys.compactMap { byId[$0] }.sorted(by: VaultModel.order)
            reused = health.reused
                .map { $0.compactMap { byId[$0] }.sorted(by: VaultModel.order) }
                .filter { $0.count > 1 }
            loaded = true
            error = nil
        } catch {
            self.error = message(for: error)
            return
        }
        guard breachCheckOn else { return }
        checkingLeaks = true
        defer { checkingLeaks = false }
        do {
            let counts = try await vault.account.breachedPasswords()
            leaked = counts.compactMap { id, count in byId[id].map { Leaked(entry: $0, count: count) } }
                .sorted { VaultModel.order($0.entry, $1.entry) }
            checkedLeaks = true
        } catch {
            self.error = message(for: error)
        }
    }

    /// Fixing an item opens its edit screen, with Generate.
    public func edit(_ entry: Entry) -> ItemEditModel {
        vault.edit(entry)
    }
}

/// The account's devices: rename any, revoke any but this one.
@MainActor
@Observable
public final class DevicesModel {
    public private(set) var devices: [DeviceEntry] = []
    public private(set) var loaded = false
    public private(set) var busy = false
    public private(set) var error: String?

    private let account: any Account

    init(account: any Account) {
        self.account = account
    }

    public func load() async {
        do {
            devices = try await account.devices()
            loaded = true
            error = nil
        } catch {
            self.error = message(for: error)
        }
    }

    /// Returns whether it was renamed.
    @discardableResult
    public func rename(_ device: DeviceEntry, to name: String) async -> Bool {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, device.revoked == nil else { return false }
        return await change { try await self.account.renameDevice(device.id, to: name) }
    }

    /// Signs another device out for good; the screen asks first. This
    /// device uses Sign out instead.
    public func revoke(_ device: DeviceEntry) async {
        guard !device.isCurrent, device.revoked == nil else { return }
        await change { try await self.account.revokeDevice(device.id) }
    }

    @discardableResult
    private func change(_ action: @escaping () async throws -> Void) async -> Bool {
        busy = true
        defer { busy = false }
        do {
            try await action()
            await load()
            return true
        } catch {
            self.error = message(for: error)
            return false
        }
    }
}

/// Export (D24): the master password every time, then an encrypted file
/// with its own password, or a plaintext one after a warning. Every plan.
@MainActor
@Observable
public final class ExportModel {
    public enum Choice: String, CaseIterable, Sendable {
        case encrypted, json, csv
    }

    public var choice: Choice = .encrypted
    public var masterPassword = ""
    public var exportPassword = ""
    public var confirm = ""
    /// The plaintext warning was read.
    public var understood = false
    public private(set) var strength: Generator.Strength?
    public private(set) var file: Redacted<Data>?
    public private(set) var fileName = ""
    public private(set) var busy = false
    public private(set) var error: String?

    private let account: any Account
    private let today: @MainActor () -> Date

    init(account: any Account, today: @escaping @MainActor () -> Date = { Date() }) {
        self.account = account
        self.today = today
    }

    public var passwordsMatch: Bool { !confirm.isEmpty && exportPassword == confirm }
    public var strongEnough: Bool { (strength?.score ?? 0) >= CreateAccountModel.minimumScore }

    public var canExport: Bool {
        guard !busy, !masterPassword.isEmpty else { return false }
        switch choice {
        case .encrypted: return strongEnough && passwordsMatch
        case .json, .csv: return understood
        }
    }

    public func updateStrength() async {
        let password = Redacted(exportPassword)
        guard !password.value.isEmpty else {
            strength = nil
            return
        }
        let scored = await Task.detached { Generator.strength(of: password) }.value
        if password.value == exportPassword { strength = scored }
    }

    /// Checks the master password, then makes the file for the share sheet.
    public func export() async {
        guard canExport else { return }
        busy = true
        error = nil
        defer { busy = false }
        let format: ExportFile.Format = switch choice {
        case .encrypted: .pwmx(password: Redacted(exportPassword))
        case .json: .json
        case .csv: .csv
        }
        do {
            try await account.authorizeExport(password: Redacted(masterPassword))
            masterPassword = ""
            file = try await account.export(format)
            exportPassword = ""
            confirm = ""
            let formatter = ISO8601DateFormatter()
            formatter.formatOptions = [.withFullDate]
            formatter.timeZone = .current
            fileName = "PWM export \(formatter.string(from: today())).\(format.fileExtension)"
        } catch {
            masterPassword = ""
            self.error = message(for: error)
        }
    }

    /// The file is dropped once shared, or when the screen goes.
    public func done() {
        file = nil
        understood = false
    }
}

/// "Approve a new device" (D46): the account's requests; approving one
/// shows the code to compare, and the Secret Key goes only after the user
/// says the codes match.
@MainActor
@Observable
public final class ApprovalsModel {
    public private(set) var requests: [ApprovalEntry] = []
    /// The request this device accepted.
    public private(set) var approving: String?
    public private(set) var loaded = false
    public private(set) var busy = false
    public private(set) var done = false
    public private(set) var error: String?

    private let account: any Account
    /// How often the list is read while waiting for the new device's key.
    let pollEvery: Duration

    init(account: any Account, pollEvery: Duration = .seconds(2)) {
        self.account = account
        self.pollEvery = pollEvery
    }

    /// The code for the request being approved, once the new device revealed its key.
    public var code: String? {
        requests.first { $0.id == approving }?.code
    }

    public func load() async {
        do {
            requests = try await account.approvalRequests()
            loaded = true
            if let approving, !requests.contains(where: { $0.id == approving }) {
                self.approving = nil
                error = PWMError(code: "unknown_approval").message
            }
        } catch {
            self.error = message(for: error)
        }
    }

    public func approve(_ request: ApprovalEntry) async {
        error = nil
        done = false
        await change { try await self.account.acceptApproval(request.id) }
        if error == nil { approving = request.id }
    }

    /// Reads the list until the code shows (the screen keeps calling it).
    public func waitForCode() async {
        while approving != nil, code == nil, !Task.isCancelled {
            await load()
            if code != nil { return }
            try? await Task.sleep(for: pollEvery)
        }
    }

    /// The user saw the same code on both devices.
    public func codesMatch() async {
        guard let id = approving, code != nil else { return }
        await change { try await self.account.completeApproval(id) }
        if error == nil {
            approving = nil
            done = true
        }
        await load()
    }

    /// The codes differ, or the user doesn't know this request.
    public func deny(_ id: String) async {
        await change { try await self.account.denyApproval(id) }
        if approving == id { approving = nil }
        await load()
    }

    @discardableResult
    private func change(_ action: @escaping () async throws -> Void) async -> Bool {
        busy = true
        defer { busy = false }
        do {
            try await action()
            return true
        } catch {
            self.error = message(for: error)
            return false
        }
    }
}

