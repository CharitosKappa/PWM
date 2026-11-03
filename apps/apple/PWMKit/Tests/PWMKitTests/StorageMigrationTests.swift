import Foundation
import XCTest
@testable import PWMKit

/// The one-time move into the places shared with the AutoFill extension
/// (D47 §2), with real files in a temporary folder and keys in memory.
final class StorageMigrationTests: XCTestCase {
    var folder: URL!
    var oldCache: String!
    var newCache: String!

    override func setUpWithError() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("pwm-migration-\(UUID().uuidString)")
        let old = folder.appendingPathComponent("support/PWM")
        let new = folder.appendingPathComponent("group/PWM")
        for dir in [old, new] {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        oldCache = old.appendingPathComponent("vault.sqlite").path
        newCache = new.appendingPathComponent("vault.sqlite").path
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: folder)
    }

    func write(_ path: String, _ text: String) throws {
        try Data(text.utf8).write(to: URL(fileURLWithPath: path))
    }

    func read(_ path: String) -> String? {
        FileManager.default.contents(atPath: path).map { String(decoding: $0, as: UTF8.self) }
    }

    /// An old install: the cache with its write-ahead log, every key, and
    /// Face ID on with a Secure Enclave key outside the shared group.
    func oldInstall() throws -> (MemoryKeyStore, FakeBiometrics) {
        try write(oldCache, "cache")
        try write(oldCache + "-wal", "wal")
        let keys = MemoryKeyStore()
        for name in KeyName.allCases {
            keys.set(name, "value of \(name.rawValue)")
        }
        let biometrics = FakeBiometrics()
        _ = try biometrics.wrap(Redacted(Data([1, 2, 3])))
        return (keys, biometrics)
    }

    func assertMoved(_ old: MemoryKeyStore, _ new: MemoryKeyStore, _ biometrics: FakeBiometrics) {
        XCTAssertEqual(read(newCache), "cache")
        XCTAssertEqual(read(newCache + "-wal"), "wal")
        XCTAssertFalse(FileManager.default.fileExists(atPath: oldCache))
        XCTAssertFalse(FileManager.default.fileExists(atPath: oldCache + "-wal"))
        XCTAssertFalse(FileManager.default.fileExists(atPath: newCache + "-shm"), "nothing made up")
        for name in KeyName.allCases {
            XCTAssertNil(old.get(name), "\(name) left behind")
            if name == .biometricKey {
                XCTAssertNil(new.get(name), "the wrapped key is made again at the next password unlock")
            } else {
                XCTAssertEqual(new.get(name), "value of \(name.rawValue)", "\(name)")
            }
        }
        XCTAssertFalse(biometrics.hasKey, "the old Secure Enclave key is deleted")
    }

    func testMovesTheCacheAndKeysOnce() throws {
        let (old, biometrics) = try oldInstall()
        let new = MemoryKeyStore()
        try StorageMigration.run(oldCache: oldCache, newCache: newCache, oldKeys: old, newKeys: new, oldBiometrics: biometrics)
        assertMoved(old, new, biometrics)

        // Later launches change nothing, even after the core wrote more.
        try write(newCache, "cache, later")
        new.set(.accessToken, "newer token")
        try StorageMigration.run(oldCache: oldCache, newCache: newCache, oldKeys: old, newKeys: new, oldBiometrics: biometrics)
        XCTAssertEqual(read(newCache), "cache, later")
        XCTAssertEqual(new.get(.accessToken), "newer token")
    }

    func testARunStoppedHalfwayIsFinishedByTheNext() throws {
        let (old, biometrics) = try oldInstall()
        let new = MemoryKeyStore()
        // Stopped after the log moved, before the main file.
        let stopping = StoppingMover(stopAfter: 1)
        XCTAssertThrowsError(try StorageMigration.run(
            oldCache: oldCache, newCache: newCache, oldKeys: old, newKeys: new, oldBiometrics: biometrics, files: stopping
        ))
        XCTAssertTrue(FileManager.default.fileExists(atPath: oldCache), "the main file is still in the old place")
        XCTAssertEqual(read(newCache + "-wal"), "wal")

        // Stopped again, this time among the keys.
        let failing = FailingKeyStore(inner: new, failAfter: 3)
        XCTAssertThrowsError(try StorageMigration.run(
            oldCache: oldCache, newCache: newCache, oldKeys: old, newKeys: failing, oldBiometrics: biometrics
        ))
        XCTAssertEqual(read(newCache), "cache")
        XCTAssertNotNil(old.get(.email), "keys not copied yet stay in the old place")

        try StorageMigration.run(oldCache: oldCache, newCache: newCache, oldKeys: old, newKeys: new, oldBiometrics: biometrics)
        assertMoved(old, new, biometrics)
    }

    func testAFreshInstallHasNothingToMove() throws {
        let old = MemoryKeyStore()
        let new = MemoryKeyStore()
        let biometrics = FakeBiometrics()
        try StorageMigration.run(oldCache: oldCache, newCache: newCache, oldKeys: old, newKeys: new, oldBiometrics: biometrics)
        XCTAssertFalse(FileManager.default.fileExists(atPath: newCache))
        XCTAssertTrue(KeyName.allCases.allSatisfy { new.get($0) == nil })
    }

    // MARK: Suggestions (D47 §5)

    func testRecordsNameTheVaultAndItem() {
        let record = Suggestion.record(vaultId: "v1", itemId: "i1")
        XCTAssertEqual(Suggestion.parse(record: record)?.vaultId, "v1")
        XCTAssertEqual(Suggestion.parse(record: record)?.itemId, "i1")
        for bad in ["", "v1", "/i1", "v1/", "a/b/c"] {
            XCTAssertNil(Suggestion.parse(record: bad), bad)
        }
    }

    func testTheLatestListWinsAndTheSwitchEmptiesIt() async throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "pwm-suggestions-\(UUID().uuidString)"))
        let store = FakeSuggestionStore()
        let suggestions = Suggestions(store: store, defaults: defaults)
        XCTAssertTrue(suggestions.isOn, "on by default")
        let one = Suggestion(kind: .password, service: "example.com", user: "ada", record: "v/1")
        let two = Suggestion(kind: .password, service: "example.org", user: "bob", record: "v/2")
        suggestions.update([one])
        suggestions.update([one, two])
        await suggestions.settle()
        let last = await store.lists.last
        XCTAssertEqual(last, [one, two])

        suggestions.isOn = false
        suggestions.update([one, two])
        await suggestions.settle()
        let afterOff = await store.lists.last
        XCTAssertEqual(afterOff, [], "off sends an empty list")
        XCTAssertFalse(Suggestions(store: store, defaults: defaults).isOn, "kept")

        suggestions.isOn = true
        suggestions.update([two])
        suggestions.clear()
        await suggestions.settle()
        let cleared = await store.lists.last
        XCTAssertEqual(cleared, [], "signing out clears it")
    }
}

/// Moves files until `stopAfter` moves, then fails, like an app killed
/// halfway.
final class StoppingMover: FileMover, @unchecked Sendable {
    private let system = SystemFileMover()
    private var moves = 0
    let stopAfter: Int

    init(stopAfter: Int) {
        self.stopAfter = stopAfter
    }

    func exists(_ path: String) -> Bool { system.exists(path) }

    func move(_ path: String, to target: String) throws {
        guard moves < stopAfter else { throw PWMError(code: "storage") }
        moves += 1
        try system.move(path, to: target)
    }

    func remove(_ path: String) throws { try system.remove(path) }
}

/// Stores `failAfter` values, then fails.
final class FailingKeyStore: KeyStore, @unchecked Sendable {
    let inner: MemoryKeyStore
    private var sets = 0
    let failAfter: Int

    init(inner: MemoryKeyStore, failAfter: Int) {
        self.inner = inner
        self.failAfter = failAfter
    }

    func get(_ name: KeyName) throws -> String? { inner.get(name) }

    func set(_ name: KeyName, _ value: String) throws {
        guard sets < failAfter else { throw PWMError(code: "keychain") }
        sets += 1
        inner.set(name, value)
    }

    func delete(_ name: KeyName) throws { inner.delete(name) }
}

/// Records every list the system would get.
actor FakeSuggestionStore: SuggestionStore {
    var lists: [[Suggestion]] = []

    func replace(with suggestions: [Suggestion]) {
        lists.append(suggestions)
    }
}
