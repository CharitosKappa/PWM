import UIKit
import XCTest

/// The app in the simulator against a real local server (D42 §6, D43 §9):
/// `PWM_TEST_SERVER` is passed by xcodebuild as `TEST_RUNNER_PWM_TEST_SERVER`.
/// Skipped when it is not set.
@MainActor
final class OnboardingUITests: XCTestCase {
    let password = "correct horse battery staple 42"
    var app: XCUIApplication!

    func testCreateAnAccountUseTheVaultThenSignInAgainWithTheSecretKey() throws {
        guard let server = ProcessInfo.processInfo.environment["PWM_TEST_SERVER"] else {
            throw XCTSkip("PWM_TEST_SERVER is not set")
        }
        continueAfterFailure = false
        app = XCUIApplication()
        // Read by the app as its saved server (the launch-argument domain).
        app.launchArguments = ["-server", server]
        app.launch()

        // A previous run may have left an account on this simulator.
        if app.buttons["signOut"].waitForExistence(timeout: 5) {
            app.buttons["signOut"].tap()
        }

        // Create an account.
        let email = "ui-\(UUID().uuidString.lowercased())@example.com"
        tap(app.buttons["createAccount"])
        type(email, into: app.textFields["email"])
        paste(password, into: app.secureTextFields["password"])
        paste(password, into: app.secureTextFields["confirm"])
        tap(app.buttons["create"], once: .enabled)

        // The Secret Key, shown once.
        let shown = app.staticTexts["secretKey"]
        expect(shown, timeout: 60)
        let secretKey = shown.label
        XCTAssertTrue(secretKey.hasPrefix("A1-"), secretKey)
        XCTAssertFalse(app.buttons["saved"].isEnabled)
        tap(app.buttons["copy"])
        tap(app.buttons["saved"], once: .enabled)
        expect(app.buttons["newItem"], timeout: 30)

        // Add a login with a generated password.
        tap(app.buttons["newItem"])
        tap(app.buttons["Login"])
        type("Bank", into: app.textFields["title"])
        type("ada", into: app.textFields["username"])
        tap(app.buttons["generate"])
        expect(app.staticTexts["generated"])
        tap(app.buttons["useGenerated"], once: .enabled)
        tap(app.buttons["addURL"])
        type("https://bank.example", into: app.textFields["url"])
        tap(app.buttons["save"], once: .enabled)

        // Find it, check the password, edit it.
        let search = app.searchFields.firstMatch
        // iOS may tuck the search bar away under the title: pull it down.
        for _ in 0..<2 where !search.waitForExistence(timeout: 5) {
            app.collectionViews.firstMatch.swipeDown()
        }
        tap(search)
        search.typeText("bank")
        tap(row("Bank, ada"))
        // One stop per field; a hidden password says "hidden", never dots.
        let secret = field("password")
        expect(secret, value: "hidden")
        // Show is the stop's action for VoiceOver. XCUITest can't run
        // actions, so tap the eye: the 44-pt button left of Copy.
        secret.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 0.5))
            .withOffset(CGVector(dx: -(44 + 8 + 22), dy: 0)).tap()
        let revealed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value != 'hidden'"), object: secret)
        XCTAssertEqual(XCTWaiter().wait(for: [revealed], timeout: 10), .completed, screen())
        XCTAssertEqual((secret.value as? String)?.count, 20, "the generator's default length")
        tap(app.buttons["edit"])
        tap(app.textFields["username"])
        app.textFields["username"].typeText("2")
        tap(app.buttons["save"], once: .enabled)
        expect(field("username"), value: "ada2")

        // Trash it, then restore it.
        tap(app.buttons["trash"])
        tap(app.buttons["restore"])
        expect(app.buttons["trash"])

        // The simulator has no Secure Enclave: Settings says so (D45 §8).
        tap(app.tabBars.buttons["Settings"])
        expect(app.staticTexts["biometricUnavailable"])
        // PWM is not the AutoFill provider yet: the card says how (D47 §7).
        expect(app.buttons["openAutoFill"])

        // Lock at once on leaving the app.
        tap(scrolledTo(app.buttons["lockTimeout"]))
        tap(app.buttons["Immediately"])
        XCUIDevice.shared.press(.home)
        app.activate()
        paste(password, into: app.secureTextFields["password"])
        tap(app.buttons["unlock"], once: .enabled)
        expect(app.buttons["newItem"], timeout: 30)

        // Name this device; the server keeps the name sealed.
        tap(app.tabBars.buttons["Settings"])
        tap(scrolledTo(app.buttons["devices"]))
        let current = app.descendants(matching: .any)["device.current"]
        expect(current, timeout: 30)
        current.swipeLeft()
        tap(app.buttons["Rename"])
        let name = app.alerts.textFields.firstMatch
        tap(name)
        name.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 20) + "Test iPhone")
        tap(app.alerts.buttons["Rename"])
        let renamed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS 'Test iPhone'"), object: current)
        XCTAssertEqual(XCTWaiter().wait(for: [renamed], timeout: 30), .completed, screen())
        app.navigationBars.buttons.firstMatch.tap()

        // Sign out, then sign in again as on a new device: the login synced.
        signOut()
        tap(app.buttons["signIn"])
        type(email, into: app.textFields["email"])
        paste(password, into: app.secureTextFields["password"])
        type(secretKey, into: app.textFields["secretKey"])
        tap(app.buttons["submit"], once: .enabled)
        expect(app.staticTexts["Bank"].firstMatch, timeout: 60)

        // Lock, then unlock with the master password only.
        tap(app.tabBars.buttons["Settings"])
        tap(scrolledTo(app.buttons["lockNow"]))
        paste(password, into: app.secureTextFields["password"])
        tap(app.buttons["unlock"], once: .enabled)
        expect(row("Bank, ada"), timeout: 30)
        signOut()
        expect(app.buttons["createAccount"], timeout: 10)
    }

    /// Settings, Sign out, and the confirmation.
    private func signOut() {
        tap(app.tabBars.buttons["Settings"])
        tap(scrolledTo(app.buttons["signOut"]))
        let confirm = app.buttons["confirmSignOut"]
        if confirm.waitForExistence(timeout: 3) {
            confirm.tap()
        } else {
            tap(app.sheets.buttons["Sign out"])
        }
    }

    /// A vault row: one stop whose label starts with the name and username.
    private func row(_ label: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", label)).firstMatch
    }

    /// An item's field: one stop with its label and value.
    private func field(_ key: String) -> XCUIElement {
        app.descendants(matching: .any)["field.\(key)"].firstMatch
    }

    /// Swipes Settings up until `element` is there: rows off screen are not
    /// made yet.
    private func scrolledTo(_ element: XCUIElement) -> XCUIElement {
        for _ in 0..<8 where !element.exists {
            app.swipeUp()
        }
        return element
    }

    private enum State {
        case exists, enabled
    }

    /// Fails with what the screen shows: the error line, the controls and the tree.
    private func expect(_ element: XCUIElement, _ state: State = .exists, value: String? = nil, timeout: TimeInterval = 10) {
        var predicate = state == .exists ? "exists == true" : "exists == true AND isEnabled == true"
        if let value { predicate += " AND value == '\(value)'" }
        let done = XCTNSPredicateExpectation(predicate: NSPredicate(format: predicate), object: element)
        if XCTWaiter().wait(for: [done], timeout: timeout) != .completed {
            XCTFail("\(element) not \(state)\(value.map { " with value \($0)" } ?? "") after \(Int(timeout)) s. \(screen())")
        }
    }

    private func tap(_ element: XCUIElement, once state: State = .exists) {
        expect(element, state)
        element.tap()
    }

    private func type(_ text: String, into field: XCUIElement) {
        tap(field)
        field.typeText(text)
        expect(field, value: text)
    }

    /// Secure fields drop typed characters in the simulator, and report only
    /// "•" as their value, so a password goes in through the edit menu's
    /// Paste, as a user would paste it. The next step
    /// (the enabled button) checks that it arrived.
    private func paste(_ text: String, into field: XCUIElement) {
        UIPasteboard.general.string = text
        tap(field)
        let menu = app.menuItems["Paste"]
        let button = app.buttons["Paste"]
        for _ in 0..<3 where !menu.exists && !button.exists {
            field.tap()
            _ = menu.waitForExistence(timeout: 2) || button.waitForExistence(timeout: 1)
        }
        if menu.exists {
            menu.tap()
        } else {
            tap(button)
        }
        UIPasteboard.general.string = ""
    }

    private func screen() -> String {
        let error = app.staticTexts["error"]
        let shown = error.exists ? error.label.trimmingCharacters(in: .whitespaces) : ""
        let buttons = app.buttons.allElementsBoundByIndex
            .map { "\($0.identifier.isEmpty ? $0.label : $0.identifier)\($0.isEnabled ? "" : " (disabled)")" }
        let texts = app.staticTexts.allElementsBoundByIndex.map(\.label)
            .filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
        let strength = app.staticTexts["strength"]
        let meter = strength.exists ? (strength.value as? String) ?? "?" : "none"
        return "Error line: \"\(shown)\". Buttons: \(buttons.joined(separator: ", ")). "
            + "Texts: \(texts.joined(separator: " | ")). Strength: \(meter).\nTree:\n\(app.debugDescription)"
    }
}
