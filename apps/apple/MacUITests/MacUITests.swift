import XCTest

/// The macOS app against a real local server (D48 §6): sign up, add a
/// login from the menu bar, lock and unlock, sign out. `PWM_TEST_SERVER` is
/// passed by xcodebuild as `TEST_RUNNER_PWM_TEST_SERVER`; skipped without it.
@MainActor
final class MacUITests: XCTestCase {
    let password = "correct horse battery staple 42"
    var app: XCUIApplication!

    func testSignUpAddALoginLockUnlockAndSignOut() throws {
        guard let server = ProcessInfo.processInfo.environment["PWM_TEST_SERVER"] else {
            throw XCTSkip("PWM_TEST_SERVER is not set")
        }
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-server", server]
        app.launch()

        // A previous run may have left an account on this Mac.
        if element("sidebar.settings").waitForExistence(timeout: 5) {
            signOut()
        }

        // Create an account.
        let email = "mac-\(UUID().uuidString.lowercased())@example.com"
        click(app.buttons["createAccount"])
        type(email, into: app.textFields["email"])
        type(password, into: app.secureTextFields["password"])
        type(password, into: app.secureTextFields["confirm"])
        click(app.buttons["create"], once: .enabled)

        // The Secret Key, shown once.
        let shown = app.staticTexts["secretKey"]
        expect(shown, timeout: 60)
        XCTAssertTrue(shown.label.hasPrefix("A1-") || (shown.value as? String)?.hasPrefix("A1-") == true)
        click(app.buttons["copy"])
        click(app.buttons["saved"], once: .enabled)
        expect(element("sidebar.settings"), timeout: 30)

        // ⌘N: a new login.
        app.typeKey("n", modifierFlags: .command)
        type("Bank", into: app.textFields["title"])
        type("ada", into: app.textFields["username"])
        click(app.buttons["save"], once: .enabled)
        expect(app.staticTexts["Bank"].firstMatch)

        // ⇧⌘L locks; the master password unlocks.
        app.typeKey("l", modifierFlags: [.command, .shift])
        type(password, into: app.secureTextFields["password"])
        click(app.buttons["unlock"], once: .enabled)
        expect(app.staticTexts["Bank"].firstMatch, timeout: 30)

        signOut()
        expect(app.buttons["createAccount"])
    }

    private func signOut() {
        click(element("sidebar.settings"))
        let button = app.buttons["signOut"]
        for _ in 0..<6 where !button.isHittable {
            app.scrollViews.firstMatch.scroll(byDeltaX: 0, deltaY: -600)
        }
        click(button)
        click(app.buttons["confirmSignOut"].firstMatch)
    }

    private func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any)[identifier].firstMatch
    }

    private enum State {
        case exists, enabled
    }

    /// Fails with what the window shows.
    private func expect(_ element: XCUIElement, _ state: State = .exists, timeout: TimeInterval = 10) {
        let predicate = state == .exists ? "exists == true" : "exists == true AND isEnabled == true"
        let done = XCTNSPredicateExpectation(predicate: NSPredicate(format: predicate), object: element)
        if XCTWaiter().wait(for: [done], timeout: timeout) != .completed {
            XCTFail("\(element) not \(state) after \(Int(timeout)) s.\n\(app.debugDescription)")
        }
    }

    private func click(_ element: XCUIElement, once state: State = .exists) {
        expect(element, state)
        element.click()
    }

    private func type(_ text: String, into field: XCUIElement) {
        click(field)
        field.typeText(text)
    }
}
