import XCTest

/// The onboarding flow in the simulator against a real local server
/// (D42 §6): `PWM_TEST_SERVER` is passed by xcodebuild as
/// `TEST_RUNNER_PWM_TEST_SERVER`. Skipped when it is not set.
@MainActor
final class OnboardingUITests: XCTestCase {
    let password = "correct horse battery staple 42"
    var app: XCUIApplication!

    func testCreateAnAccountThenSignInAgainWithTheSecretKey() throws {
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
        type(password, into: app.secureTextFields["password"])
        type(password + "\n", into: app.secureTextFields["confirm"])
        tap(app.buttons["create"], once: .enabled)

        // The Secret Key, shown once.
        let shown = app.staticTexts["secretKey"]
        expect(shown, timeout: 60)
        let secretKey = shown.label
        XCTAssertTrue(secretKey.hasPrefix("A1-"), secretKey)
        XCTAssertFalse(app.buttons["saved"].isEnabled)
        tap(app.buttons["copy"])
        tap(app.buttons["saved"], once: .enabled)
        expect(app.staticTexts["itemCount"], timeout: 30)

        // Sign out, then sign in again as on a new device.
        tap(app.buttons["signOut"])
        tap(app.buttons["signIn"])
        type(email, into: app.textFields["email"])
        type(password, into: app.secureTextFields["password"])
        type(secretKey + "\n", into: app.textFields["secretKey"])
        tap(app.buttons["submit"], once: .enabled)
        expect(app.staticTexts["itemCount"], timeout: 60)

        // Lock, then unlock with the master password only.
        tap(app.buttons["lock"])
        type(password, into: app.secureTextFields["password"])
        tap(app.buttons["unlock"], once: .enabled)
        expect(app.staticTexts["itemCount"], timeout: 30)
        tap(app.buttons["signOut"])
        expect(app.buttons["createAccount"], timeout: 10)
    }

    private enum State {
        case exists, enabled
    }

    /// Fails with what the screen shows: the error line and the controls.
    private func expect(_ element: XCUIElement, _ state: State = .exists, timeout: TimeInterval = 10) {
        let predicate = state == .exists ? "exists == true" : "exists == true AND isEnabled == true"
        let done = XCTNSPredicateExpectation(predicate: NSPredicate(format: predicate), object: element)
        if XCTWaiter().wait(for: [done], timeout: timeout) != .completed {
            XCTFail("\(element) not \(state) after \(Int(timeout)) s. \(screen())")
        }
    }

    private func tap(_ element: XCUIElement, once state: State = .exists) {
        expect(element, state)
        element.tap()
    }

    private func type(_ text: String, into field: XCUIElement) {
        tap(field)
        field.typeText(text)
    }

    private func screen() -> String {
        let error = app.staticTexts["error"]
        let shown = error.exists ? error.label.trimmingCharacters(in: .whitespaces) : ""
        let controls = (app.buttons.allElementsBoundByIndex + app.textFields.allElementsBoundByIndex
            + app.secureTextFields.allElementsBoundByIndex)
            .map { "\($0.identifier.isEmpty ? $0.label : $0.identifier)\($0.isEnabled ? "" : " (disabled)")" }
        return "Error line: \"\(shown)\". Controls: \(controls.joined(separator: ", "))."
    }
}
