import XCTest

/// The onboarding flow in the simulator against a real local server
/// (D42 §6): `PWM_TEST_SERVER` is passed by xcodebuild as
/// `TEST_RUNNER_PWM_TEST_SERVER`. Skipped when it is not set.
@MainActor
final class OnboardingUITests: XCTestCase {
    let password = "correct horse battery staple 42"

    func testCreateAnAccountThenSignInAgainWithTheSecretKey() throws {
        guard let server = ProcessInfo.processInfo.environment["PWM_TEST_SERVER"] else {
            throw XCTSkip("PWM_TEST_SERVER is not set")
        }
        continueAfterFailure = false
        let app = XCUIApplication()
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
        let create = app.buttons["create"]
        wait(for: create, enabled: true)
        create.tap()

        // The Secret Key, shown once.
        let shown = app.staticTexts["secretKey"]
        XCTAssertTrue(shown.waitForExistence(timeout: 60))
        let secretKey = shown.label
        XCTAssertTrue(secretKey.hasPrefix("A1-"), secretKey)
        XCTAssertFalse(app.buttons["saved"].isEnabled)
        tap(app.buttons["copy"])
        wait(for: app.buttons["saved"], enabled: true)
        app.buttons["saved"].tap()
        XCTAssertTrue(app.staticTexts["itemCount"].waitForExistence(timeout: 30))

        // Sign out, then sign in again as on a new device.
        tap(app.buttons["signOut"])
        tap(app.buttons["signIn"])
        type(email, into: app.textFields["email"])
        type(password, into: app.secureTextFields["password"])
        type(secretKey + "\n", into: app.textFields["secretKey"])
        let submit = app.buttons["submit"]
        wait(for: submit, enabled: true)
        submit.tap()
        XCTAssertTrue(app.staticTexts["itemCount"].waitForExistence(timeout: 60))

        // Lock, then unlock with the master password only.
        tap(app.buttons["lock"])
        type(password + "\n", into: app.secureTextFields["password"])
        if !app.staticTexts["itemCount"].waitForExistence(timeout: 30) {
            tap(app.buttons["unlock"])
        }
        XCTAssertTrue(app.staticTexts["itemCount"].waitForExistence(timeout: 30))
        tap(app.buttons["signOut"])
        XCTAssertTrue(app.buttons["createAccount"].waitForExistence(timeout: 10))
    }

    private func tap(_ element: XCUIElement) {
        XCTAssertTrue(element.waitForExistence(timeout: 10), element.debugDescription)
        element.tap()
    }

    private func type(_ text: String, into field: XCUIElement) {
        tap(field)
        field.typeText(text)
    }

    private func wait(for element: XCUIElement, enabled: Bool) {
        let done = expectation(for: NSPredicate(format: "isEnabled == %@", NSNumber(value: enabled)), evaluatedWith: element)
        wait(for: [done], timeout: 30)
    }
}
