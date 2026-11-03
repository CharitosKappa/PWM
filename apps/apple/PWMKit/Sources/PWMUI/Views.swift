// The onboarding screens (D42 §4), shared by the iOS app and, with M5.6, the
// macOS app. Copy is plain and short; nothing moves under the cursor. The
// vault's screens are in VaultViews.swift.
#if canImport(SwiftUI)
import Combine
import PWMDesign
import PWMKit
import SwiftUI
import UniformTypeIdentifiers
#if os(iOS)
import UIKit
#elseif os(macOS)
import AppKit
#endif

/// The app's root: one screen at a time, chosen by `AppModel`. It tells the
/// model when the app leaves and comes back, for the lock timeout (D43 §5),
/// and covers the vault and the Secret Key from leaving until that check is
/// done. On iOS the cover starts as soon as the app is inactive, before the
/// app switcher takes its picture (D49 §4); it also feeds `ScreenCapture`.
public struct RootView: View {
    let model: AppModel
    @Environment(\.scenePhase) private var scenePhase
    @State private var covered = false
    @State private var active = true
    /// The lock timeout check after coming back is under way.
    @State private var checking = false

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some View {
        screens
            .overlay {
                if covered && (model.screen == .vault || model.screen == .secretKey) {
                    Tokens.Color.bg.color.ignoresSafeArea()
                        .accessibilityIdentifier("privacyCover")
                }
            }
            .onChange(of: scenePhase) { old, new in
                active = new == .active
                switch new {
                case .background:
                    covered = true
                    Task { await model.didEnterBackground() }
                case .inactive:
                    #if os(iOS)
                    covered = true
                    #endif
                    if old == .background { comeBack() }
                case .active:
                    if old == .background { comeBack() } else if !checking { covered = false }
                @unknown default:
                    break
                }
            }
            #if os(iOS)
            .background(CaptureWatcher())
            .onReceive(NotificationCenter.default.publisher(for: UIApplication.userDidTakeScreenshotNotification)) { _ in
                ScreenCapture.shared.screenshotTaken()
            }
            .onReceive(NotificationCenter.default.publisher(for: UIApplication.protectedDataWillBecomeUnavailableNotification)) { _ in
                Task { await model.deviceWillLock() }
            }
            #elseif os(macOS)
            // The Mac sleeps or its screen locks: the vault locks (D48 §2).
            // Switching to another app counts as leaving, for the timeout.
            .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.willSleepNotification)) { _ in
                Task { await model.deviceWillLock() }
            }
            .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.screensDidSleepNotification)) { _ in
                Task { await model.deviceWillLock() }
            }
            .onReceive(DistributedNotificationCenter.default().publisher(for: Notification.Name("com.apple.screenIsLocked"))) { _ in
                Task { await model.deviceWillLock() }
            }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)) { _ in
                Task { await model.didEnterBackground() }
            }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
                Task { await model.willEnterForeground() }
            }
            .background(WindowSharing(hidden: model.hidesWindow))
            #endif
    }

    /// Back from the background: the lock timeout first, then uncover (on
    /// iOS only once active again, so the switcher never sees the vault).
    private func comeBack() {
        checking = true
        Task {
            await model.willEnterForeground()
            checking = false
            #if os(iOS)
            if active { covered = false }
            #else
            covered = false
            #endif
        }
    }

    private var screens: some View {
        Group {
            switch model.screen {
            case .loading:
                ProgressView()
            case .welcome:
                WelcomeView(model: model)
            case .createAccount:
                if let create = model.createAccount { CreateAccountView(model: create, app: model) }
            case .secretKey:
                if let key = model.secretKey { SecretKeyView(model: key) }
            case .signIn:
                if let signIn = model.signIn { SignInView(model: signIn, app: model) }
            case .unlock:
                if let unlock = model.unlock { UnlockView(model: unlock) }
            case .vault:
                MainView(app: model)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Tokens.Color.bg.color)
        .foregroundStyle(Tokens.Color.text.color)
        .tint(Tokens.Color.accent.color)
    }
}

/// A screen's frame: a title, a short line under it, then the content.
struct Page<Content: View>: View {
    let title: LocalizedStringKey
    let subtitle: LocalizedStringKey?
    @ViewBuilder let content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Tokens.Space.s4) {
                Text(title).textStyle(Tokens.Text.display)
                if let subtitle {
                    Text(subtitle).textStyle(Tokens.Text.body).foregroundStyle(Tokens.Color.textMuted.color)
                }
                content
            }
            .padding(Tokens.Space.s5)
            .frame(maxWidth: 480, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }
}

/// An error line, kept in the layout when empty so nothing shifts.
struct ErrorText: View {
    let text: String?

    var body: some View {
        Text(verbatim: text ?? " ")
            .textStyle(Tokens.Text.caption)
            .foregroundStyle(Tokens.Color.danger.color)
            .accessibilityHidden(text == nil)
            .accessibilityIdentifier("error")
    }
}

struct PrimaryButton: View {
    let title: LocalizedStringKey
    let enabled: Bool
    let action: () async -> Void

    var body: some View {
        Button {
            Task { await action() }
        } label: {
            Text(title).textStyle(Tokens.Text.bodyStrong).frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent)
        .controlSize(.large)
        .disabled(!enabled)
    }
}

/// No `newPassword` on purpose: iOS would offer to generate the master
/// password and keep it in iCloud Keychain, and it must be one the user knows.
enum FieldKind {
    case email, url, password
}

extension View {
    /// Plain text input: no autocorrection or capitalization.
    func plainInput() -> some View {
        #if os(iOS)
        self.textInputAutocapitalization(.never).autocorrectionDisabled()
        #else
        self.autocorrectionDisabled()
        #endif
    }

    /// What the field holds, for the keyboard and Password AutoFill.
    @ViewBuilder
    func field(_ kind: FieldKind) -> some View {
        #if os(iOS)
        switch kind {
        case .email: self.textContentType(.emailAddress).keyboardType(.emailAddress)
        case .url: self.textContentType(.URL).keyboardType(.URL)
        case .password: self.textContentType(.password)
        }
        #elseif os(macOS)
        switch kind {
        case .password: self.textContentType(.password)
        case .email, .url: self
        }
        #else
        self
        #endif
    }
}

// MARK: Welcome

struct WelcomeView: View {
    @Bindable var model: AppModel

    var body: some View {
        Page(title: "Welcome", subtitle: "Your passwords, encrypted on your devices before they are synced.") {
            PrimaryButton(title: "Create account", enabled: true) { model.showCreateAccount() }
                .accessibilityIdentifier("createAccount")
            Button("Sign in") { model.showSignIn() }
                .frame(maxWidth: .infinity)
                .controlSize(.large)
                .accessibilityIdentifier("signIn")
            DisclosureGroup("Other server", isExpanded: $model.usesOtherServer) {
                TextField("https://vault.example.com", text: $model.otherServer)
                    .plainInput()
                    .field(.url)
                    .accessibilityIdentifier("server")
                Text("For a self-hosted server. Leave closed to use ours.")
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
            }
            ErrorText(text: model.error)
        }
    }
}

// MARK: Create account

struct CreateAccountView: View {
    @Bindable var model: CreateAccountModel
    let app: AppModel

    var body: some View {
        Page(title: "Create account", subtitle: "Nobody, including us, can reset your master password.") {
            TextField("Email", text: $model.email)
                .plainInput()
                .field(.email)
                .accessibilityIdentifier("email")
            SecureField("Master password", text: $model.password)
                .accessibilityIdentifier("password")
            StrengthMeter(strength: model.strength)
            SecureField("Master password again", text: $model.confirm)
                .accessibilityIdentifier("confirm")
            Text(!model.confirm.isEmpty && !model.passwordsMatch ? "The passwords don't match." : " ")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.danger.color)
            PrimaryButton(title: model.busy ? "Creating…" : "Create account", enabled: model.canSubmit) {
                await model.submit()
            }
            .accessibilityIdentifier("create")
            Button("Back") { app.showWelcome() }.disabled(model.busy)
            ErrorText(text: model.error)
        }
        .textFieldStyle(.roundedBorder)
        .task(id: model.password + "\u{0}" + model.email) { await model.updateStrength() }
    }
}

struct StrengthMeter: View {
    let strength: Generator.Strength?

    private static let words = ["Too guessable", "Weak", "Fair", "Strong", "Very strong"]

    var body: some View {
        let score = strength?.score ?? -1
        VStack(alignment: .leading, spacing: Tokens.Space.s1) {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(Tokens.Color.border.color)
                    Capsule()
                        .fill(score >= CreateAccountModel.minimumScore ? Tokens.Color.success.color : Tokens.Color.warning.color)
                        .frame(width: geometry.size.width * Double(score + 1) / 5)
                }
            }
            .frame(height: 4)
            Text(strength.map { LocalizedStringKey(Self.words[min(max($0.score, 0), 4)]) } ?? " ")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Password strength")
        .accessibilityIdentifier("strength")
        .accessibilityValue(strength.map { Self.words[min(max($0.score, 0), 4)] } ?? "Empty")
    }
}

// MARK: Secret Key

/// The kit PDF for the share sheet, as data: no file of the app's own.
struct KitFile: Transferable {
    let data: Data

    static var transferRepresentation: some TransferRepresentation {
        DataRepresentation(exportedContentType: .pdf) { $0.data }
            .suggestedFileName("Emergency Kit.pdf")
    }
}

struct SecretKeyView: View {
    let model: SecretKeyModel

    var body: some View {
        Page(title: "Save your Secret Key", subtitle: "You need it with your master password to sign in on a new device. It is shown only now.") {
            Text(verbatim: model.hiddenByCapture ? ScreenCapture.hiddenLine : model.secretKey.value)
                .textStyle(Tokens.Text.secret)
                .textSelection(.enabled)
                .padding(Tokens.Space.s3)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Tokens.Color.surface.color, in: RoundedRectangle(cornerRadius: Tokens.Radius.md))
                .accessibilityIdentifier("secretKey")
            HStack(spacing: Tokens.Space.s3) {
                if let kit = model.kit {
                    ShareLink(item: KitFile(data: kit.value), preview: SharePreview("Emergency Kit")) {
                        Label("Save Emergency Kit", systemImage: "square.and.arrow.down")
                    }
                    .simultaneousGesture(TapGesture().onEnded { model.didShareKit() })
                    .accessibilityIdentifier("saveKit")
                } else {
                    ProgressView()
                }
                Button(model.copied ? "Copied" : "Copy") { model.copy() }
                    .accessibilityIdentifier("copy")
            }
            .buttonStyle(.bordered)
            Text(model.copied ? "The clipboard clears in 30 seconds." : "Print it or save it somewhere safe and offline.")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
            PrimaryButton(title: "I saved it", enabled: model.canContinue) { model.done() }
                .accessibilityIdentifier("saved")
            ErrorText(text: model.error)
        }
        .task { await model.prepareKit() }
    }
}

// MARK: Sign in

struct SignInView: View {
    @Bindable var model: SignInModel
    @Bindable var app: AppModel
    @State private var scanning = false

    var body: some View {
        Page(title: "Sign in", subtitle: "Use your master password and the Secret Key on your Emergency Kit.") {
            if QRScanner.isAvailable {
                Button {
                    scanning = true
                } label: {
                    Label("Scan Emergency Kit", systemImage: "qrcode.viewfinder").frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
            }
            TextField("Email", text: $model.email)
                .plainInput()
                .field(.email)
                .accessibilityIdentifier("email")
            if model.asking {
                AskingPanel(model: model)
            } else if !model.approved {
                Button {
                    model.startAsking()
                } label: {
                    Label("Sign in with another device", systemImage: "iphone.and.arrow.forward").frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
                .disabled(!model.canAsk)
                .accessibilityIdentifier("askDevice")
            }
            SecureField("Master password", text: $model.password)
                .field(.password)
                .accessibilityIdentifier("password")
            if model.approved {
                Label("Secret Key received from your other device.", systemImage: "checkmark.seal")
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.success.color)
                    .accessibilityIdentifier("approved")
            } else {
                TextField("Secret Key", text: $model.secretKey)
                    .plainInput()
                    .font(.system(.body, design: .monospaced))
                    .accessibilityIdentifier("secretKey")
            }
            if app.usesOtherServer {
                Text("Server: \(app.otherServer)")
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
            }
            PrimaryButton(title: model.busy ? "Signing in…" : "Sign in", enabled: model.canSubmit) {
                await model.submit()
            }
            .accessibilityIdentifier("submit")
            Button("Back") { app.showWelcome() }.disabled(model.busy)
            ErrorText(text: model.error)
        }
        .textFieldStyle(.roundedBorder)
        .sheet(isPresented: $scanning) {
            QRScanner { text in
                if model.scanned(text) { scanning = false }
            }
            .ignoresSafeArea()
        }
    }
}

// MARK: Unlock and vault

extension BiometricKind {
    var label: LocalizedStringKey {
        switch self {
        case .faceID: "Face ID"
        case .touchID: "Touch ID"
        }
    }

    var symbol: String {
        switch self {
        case .faceID: "faceid"
        case .touchID: "touchid"
        }
    }
}

struct UnlockView: View {
    @Bindable var model: UnlockModel
    /// Face ID is tried once by itself each time this screen shows.
    @State private var triedBiometrics = false

    var body: some View {
        Page(title: "Unlock", subtitle: nil) {
            if let email = model.email {
                Text(verbatim: email).textStyle(Tokens.Text.body).foregroundStyle(Tokens.Color.textMuted.color)
            }
            if let kind = model.offer {
                BiometricOffer(kind: kind, model: model)
            } else {
                unlockFields
            }
        }
        .task {
            await model.load()
            if model.biometricKind != nil, !triedBiometrics {
                triedBiometrics = true
                await model.unlockWithBiometrics()
            }
        }
    }

    @ViewBuilder private var unlockFields: some View {
        if let kind = model.biometricKind {
            Button {
                Task { await model.unlockWithBiometrics() }
            } label: {
                Label {
                    Text("Unlock with \(Text(kind.label))")
                } icon: {
                    Image(systemName: kind.symbol)
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
            .controlSize(.large)
            .disabled(model.busy)
            .accessibilityIdentifier("unlockBiometric")
        }
        SecureField("Master password", text: $model.password)
            .textFieldStyle(.roundedBorder)
            .field(.password)
            .onSubmit { Task { await model.submit() } }
            .accessibilityIdentifier("password")
        PrimaryButton(title: model.busy ? "Unlocking…" : "Unlock", enabled: model.canSubmit) {
            await model.submit()
        }
        .accessibilityIdentifier("unlock")
        Button("Sign out", role: .destructive) { Task { await model.signOut() } }
            .accessibilityIdentifier("signOut")
        ErrorText(text: model.error)
    }
}

/// While a signed-in device is asked (D46): the code to compare there.
struct AskingPanel: View {
    let model: SignInModel

    var body: some View {
        VStack(alignment: .leading, spacing: Tokens.Space.s3) {
            Text("On a device where you're signed in, open Settings, then Approve a new device.")
                .textStyle(Tokens.Text.body)
            if let code = model.approvalCode {
                Text("Check that it shows this code:")
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.textMuted.color)
                Text(verbatim: code)
                    .font(.system(.largeTitle, design: .monospaced).weight(.semibold))
                    .accessibilityIdentifier("approvalCode")
            } else {
                HStack(spacing: Tokens.Space.s2) {
                    ProgressView()
                    Text("Waiting for your other device…").textStyle(Tokens.Text.caption)
                }
            }
            Button("Cancel", role: .cancel) { model.cancelAsking() }
                .accessibilityIdentifier("cancelAsk")
        }
        .padding(Tokens.Space.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Tokens.Color.surface.color, in: RoundedRectangle(cornerRadius: Tokens.Radius.md))
    }
}

/// Asked once, after a master password unlock (D45 §3); no nag after that.
struct BiometricOffer: View {
    let kind: BiometricKind
    let model: UnlockModel

    var body: some View {
        VStack(alignment: .leading, spacing: Tokens.Space.s3) {
            Label {
                Text("Use \(Text(kind.label)) next time?").textStyle(Tokens.Text.bodyStrong)
            } icon: {
                Image(systemName: kind.symbol)
            }
            Text("Your master password is still asked every 14 days, so you don't forget it.")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
            PrimaryButton(title: "Turn on", enabled: !model.busy) { await model.acceptOffer() }
                .accessibilityIdentifier("acceptBiometric")
            Button("Not now") { model.declineOffer() }
                .frame(maxWidth: .infinity)
                .accessibilityIdentifier("declineBiometric")
        }
        .padding(Tokens.Space.s4)
        .background(Tokens.Color.surface.color, in: RoundedRectangle(cornerRadius: Tokens.Radius.md))
    }
}

#endif
