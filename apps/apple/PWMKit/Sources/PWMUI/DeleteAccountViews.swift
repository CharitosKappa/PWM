// Deleting the account (D74 §7), from Settings.
#if canImport(SwiftUI)
import PWMDesign
import PWMKit
import SwiftUI

/// Keeps one model while the screen shows.
struct DeleteAccountHost: View {
    let settings: SettingsModel
    @State private var model: DeleteAccountModel

    init(settings: SettingsModel) {
        self.settings = settings
        _model = State(initialValue: settings.makeDeleteAccount())
    }

    var body: some View { DeleteAccountView(model: model, settings: settings) }
}

struct DeleteAccountView: View {
    @Bindable var model: DeleteAccountModel
    let settings: SettingsModel
    @Environment(\.openURL) private var openURL

    var body: some View {
        Form {
            switch model.step {
            case .intro: intro
            case .plan: planStep
            case .password: passwordStep
            }
        }
        .navigationTitle("Delete account")
        .task { await model.load() }
    }

    @ViewBuilder private var intro: some View {
        Section {
            Text("Your vaults and items will be deleted from every device and from our server. This can't be undone.")
                .accessibilityIdentifier("deleteIntro")
            NavigationLink("Export first") { ExportHost(settings: settings) }
                .accessibilityIdentifier("deleteExportFirst")
            Button("Continue") { model.next() }
                .accessibilityIdentifier("deleteContinue")
        }
    }

    @ViewBuilder private var planStep: some View {
        Section {
            if model.endsWithRefund, let plan = model.plan.subscription?.plan {
                Text("Your \(Text(verbatim: plan.label)) plan ends now, and you'll get back the unused days.")
            }
            if model.appleBills {
                Text("Apple bills this plan. Cancel it in your Apple account, or Apple will keep charging you.")
                    .accessibilityIdentifier("deleteAppleBills")
                if model.plan.canManageInStore {
                    Button("Manage subscription") { Task { await model.plan.manageInStore() } }
                } else if let url = model.plan.manageURL(for: .appStore) {
                    Button("Manage subscription") { openURL(url) }
                }
            }
            Button("Continue") { model.next() }
                .accessibilityIdentifier("deletePlanContinue")
        } header: {
            Text("Your plan")
        }
    }

    @ViewBuilder private var passwordStep: some View {
        Section {
            SecureField("Master password", text: $model.password)
                .accessibilityIdentifier("deletePassword")
                .onSubmit { Task { await model.delete() } }
            Button(model.busy ? "Deleting…" : "Delete account", role: .destructive) {
                Task { await model.delete() }
            }
            .disabled(!model.canDelete)
            .accessibilityIdentifier("deleteConfirm")
        }
        ErrorText(text: model.error)
    }
}
#endif
