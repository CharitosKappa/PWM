import AuthenticationServices
import PWMDesign
import PWMKit
import PWMUI
import SwiftUI

/// The AutoFill extension (D47, and on macOS D48 §2): a thin shell that
/// turns what the system asks into an `AutoFillRequest`, shows
/// `AutoFillView`, and hands the answer back. iOS and macOS share it.
/// It unlocks with Face ID or Touch ID only, and every key it opened is
/// dropped before it answers.
final class CredentialProviderViewController: ASCredentialProviderViewController {
    private var model: AutoFillModel?

    /// The key icon on the keyboard: the logins for these pages or apps.
    override func prepareCredentialList(for serviceIdentifiers: [ASCredentialServiceIdentifier]) {
        show(.logins(services: serviceIdentifiers.map(\.identifier)))
    }

    /// A sign-in that accepts passkeys.
    override func prepareCredentialList(
        for serviceIdentifiers: [ASCredentialServiceIdentifier],
        requestParameters: ASPasskeyCredentialRequestParameters
    ) {
        show(.passkeys(
            rpId: requestParameters.relyingPartyIdentifier,
            clientDataHash: requestParameters.clientDataHash,
            allowed: requestParameters.allowedCredentials
        ))
    }

    /// Never fills without Face ID: iOS then shows the interface below.
    override func provideCredentialWithoutUserInteraction(for credentialRequest: any ASCredentialRequest) {
        extensionContext.cancelRequest(withError: ASExtensionError(.userInteractionRequired))
    }

    /// A suggestion chosen above the keyboard.
    override func prepareInterfaceToProvideCredential(for credentialRequest: any ASCredentialRequest) {
        if let identity = credentialRequest.credentialIdentity as? ASPasswordCredentialIdentity {
            show(.login(record: identity.recordIdentifier ?? ""))
        } else if let request = credentialRequest as? ASPasskeyCredentialRequest,
                  let identity = request.credentialIdentity as? ASPasskeyCredentialIdentity {
            show(.passkey(
                record: identity.recordIdentifier ?? "", credentialId: identity.credentialID,
                rpId: identity.relyingPartyIdentifier, clientDataHash: request.clientDataHash
            ))
        } else {
            answer(.notFound)
        }
    }

    /// A site or app asks to create a passkey. Only ES256 is made (D37).
    override func prepareInterface(forPasskeyRegistration registrationRequest: any ASCredentialRequest) {
        guard let request = registrationRequest as? ASPasskeyCredentialRequest,
              let identity = request.credentialIdentity as? ASPasskeyCredentialIdentity,
              request.supportedAlgorithms.contains(.ES256)
        else {
            extensionContext.cancelRequest(withError: ASExtensionError(.failed))
            return
        }
        show(.newPasskey(
            rpId: identity.relyingPartyIdentifier, userName: identity.userName,
            userHandle: identity.userHandle, clientDataHash: request.clientDataHash
        ))
    }

    private func show(_ request: AutoFillRequest) {
        let account: any AutoFillAccount
        do {
            let places = try SharedStorage.autoFill()
            // Nothing here talks to the server: the app syncs.
            account = try Session(
                cachePath: places.cachePath, server: Server.hosted, keys: places.keys,
                biometrics: places.biometrics,
                suggestions: Suggestions(store: SystemSuggestionStore(), defaults: SharedStorage.defaults)
            )
        } catch {
            account = SignedOutAccount()
        }
        let model = AutoFillModel(request: request, account: account, pasteboard: SystemPasteboard()) { [weak self] in
            self?.answer($0)
        }
        self.model = model
        #if os(macOS)
        let host = NSHostingController(rootView: AutoFillView(model: model))
        addChild(host)
        host.view.frame = view.bounds
        host.view.autoresizingMask = [.width, .height]
        view.addSubview(host.view)
        preferredContentSize = NSSize(width: 420, height: 480)
        #else
        SystemControls.useAppFonts()
        let host = UIHostingController(rootView: AutoFillView(model: model))
        addChild(host)
        host.view.frame = view.bounds
        host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(host.view)
        host.didMove(toParent: self)
        #endif
    }

    private func answer(_ result: AutoFillResult) {
        switch result {
        case let .login(username, password):
            extensionContext.completeRequest(
                withSelectedCredential: ASPasswordCredential(user: username, password: password.value),
                completionHandler: nil
            )
        case let .passkey(signed, rpId, clientDataHash):
            let credential = ASPasskeyAssertionCredential(
                userHandle: signed.userHandle, relyingParty: rpId, signature: signed.signature,
                clientDataHash: clientDataHash, authenticatorData: signed.authenticatorData,
                credentialID: signed.credentialId
            )
            extensionContext.completeAssertionRequest(using: credential, completionHandler: nil)
        case let .newPasskey(made, rpId, clientDataHash):
            let credential = ASPasskeyRegistrationCredential(
                relyingParty: rpId, clientDataHash: clientDataHash,
                credentialID: made.credentialId, attestationObject: made.attestationObject
            )
            extensionContext.completeRegistrationRequest(using: credential, completionHandler: nil)
        case .cancelled:
            extensionContext.cancelRequest(withError: ASExtensionError(.userCanceled))
        case .notFound:
            extensionContext.cancelRequest(withError: ASExtensionError(.credentialIdentityNotFound))
        }
    }
}

/// When the shared places can't be opened: the model says to sign in.
private final class SignedOutAccount: AutoFillAccount {
    func isSignedIn() async throws -> Bool { false }
    func biometricState() async throws -> BiometricState { .unavailable(reason: "") }
    func unlockWithBiometrics(reason: String) async throws { throw PWMError(code: "not_signed_in") }
    func lock() async {}
    func fillChoices(for services: [String]) async throws -> [FillChoice] { [] }
    func allFillChoices() async throws -> [FillChoice] { [] }
    func fillCredential(_ itemId: String, in vaultId: String) async throws -> FillCredential {
        throw PWMError(code: "not_signed_in")
    }
    func currentCode(_ itemId: String, in vaultId: String) async throws -> (code: Redacted<String>, secondsLeft: UInt64)? { nil }
    func passkeyChoices(rpId: String, allowed: [Data]) async throws -> [PasskeyChoice] { [] }
    func registerPasskey(rpId: String, userName: String, userHandle: Data) async throws -> NewPasskey {
        throw PWMError(code: "not_signed_in")
    }
    func signWithPasskey(_ choice: PasskeyChoice, rpId: String, clientDataHash: Data) async throws -> PasskeySignature {
        throw PWMError(code: "not_signed_in")
    }
}
