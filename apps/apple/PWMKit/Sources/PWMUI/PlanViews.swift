// The plan in Settings (D6 §2).
#if canImport(SwiftUI)
import PWMDesign
import PWMKit
import SwiftUI

extension Plan {
    var label: String {
        switch self {
        case .free: String(localized: "Free")
        case .premium: String(localized: "Premium")
        case .family: String(localized: "Family")
        case .teams: String(localized: "Teams")
        case let .other(name): name.capitalized
        }
    }
}

extension BillingProvider {
    /// "Managed in the App Store".
    var managedText: String {
        switch self {
        case .appStore: String(localized: "Managed in the App Store")
        case .play: String(localized: "Managed in Google Play")
        case .stripe: String(localized: "Managed on the web")
        case .other: String(localized: "Managed where you bought it")
        }
    }

    /// The second subscription, and where to cancel it.
    var secondText: String {
        switch self {
        case .appStore: String(localized: "You also have a subscription in the App Store. Cancel one there or here so you aren't charged twice.")
        case .play: String(localized: "You also have a subscription in Google Play. Cancel it in Google Play so you aren't charged twice.")
        case .stripe: String(localized: "You also have a subscription on the web. Cancel one so you aren't charged twice.")
        case .other: String(localized: "You also have a second subscription. Cancel one so you aren't charged twice.")
        }
    }
}

/// What the account has, where it's managed, and the way to buy where this
/// app sells it. No nags: buying is one button, shown only on Free.
struct PlanSection: View {
    let model: PlanModel
    @Environment(\.openURL) private var openURL
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Section {
            LabeledContent("Current plan") {
                if let plan = model.plan {
                    Text(verbatim: plan.label).accessibilityIdentifier("planName")
                } else if model.error == nil {
                    ProgressView()
                }
            }
            if let subscription = model.subscription {
                SubscriptionRows(model: model, subscription: subscription, openURL: open)
            } else if model.refunded {
                Text("Your plan ended. The refund is on its way.")
                    .textStyle(Tokens.Text.caption)
                    .accessibilityIdentifier("planRefunded")
            }
            if model.plan == .free {
                buyRows
            }
            if let error = model.error {
                // Not `ErrorText`: Settings has its own line with that identifier.
                Text(verbatim: error)
                    .textStyle(Tokens.Text.caption)
                    .foregroundStyle(Tokens.Color.danger.color)
                    .accessibilityIdentifier("planError")
            }
        } header: {
            Text("Plan")
        } footer: {
            footer
        }
        .task { await model.load() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await model.returnedToApp() } }
        }
    }

    private func open(_ url: URL) {
        openURL(url)
    }

    @ViewBuilder
    private var buyRows: some View {
        if model.purchasePending {
            Text("Waiting for approval. Your plan updates once it's approved.")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
        } else if model.canBuy {
            Button {
                Task { if let url = await model.buy() { openURL(url) } }
            } label: {
                switch model.checkout {
                case .appStore:
                    if let price = model.storePrice {
                        Text("Get Premium for \(price)")
                    } else {
                        Text("Get Premium")
                    }
                case .web:
                    Text("Get Premium")
                }
            }
            .disabled(model.busy)
            .accessibilityIdentifier("buyPremium")
        }
    }

    @ViewBuilder
    private var footer: some View {
        if model.everyAccountPremium {
            Text("Every account on this server has Premium.")
        } else if model.plan == .free, model.canBuy {
            switch model.checkout {
            case .appStore:
                Text("Paid through your Apple Account. Renews until you cancel, which you can do at any time in the App Store.")
            case .web:
                Text("Opens checkout in your browser. Your plan updates here once you've paid.")
            }
        } else if model.plan == .free, model.storeUnavailable {
            Text("Premium isn't available in the App Store right now.")
        }
    }
}

/// When it renews or ends, where it's managed, and its buttons.
private struct SubscriptionRows: View {
    let model: PlanModel
    let subscription: SubscriptionEntry
    let openURL: (URL) -> Void

    private var until: String { subscription.until.formatted(date: .abbreviated, time: .omitted) }

    var body: some View {
        Group {
            if subscription.status == "past_due" {
                Text("The last payment didn't go through. Update it where you pay for the plan.")
                    .foregroundStyle(Tokens.Color.warning.color)
            } else if subscription.canceling {
                Text("Ends on \(until). You keep it until then.")
            } else {
                Text("Renews on \(until)")
            }
        }
        .textStyle(Tokens.Text.caption)
        .accessibilityIdentifier("planRenewal")

        Text(verbatim: subscription.provider.managedText)
            .textStyle(Tokens.Text.caption)
            .foregroundStyle(Tokens.Color.textMuted.color)
            .accessibilityIdentifier("planManaged")

        if model.canCancel {
            Button("Cancel plan", role: .destructive) { Task { await model.cancel() } }
                .disabled(model.busy)
                .accessibilityIdentifier("cancelPlan")
        }
        if model.canResume {
            Button("Resume plan") { Task { await model.resume() } }
                .disabled(model.busy)
                .accessibilityIdentifier("resumePlan")
        }
        if model.resumesElsewhere {
            Text("To keep it, resume it in PWM on your computer.")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
        }
        if subscription.provider == .appStore || subscription.provider == .play {
            storeManagement(subscription.provider)
        }
        refundRows

        ForEach(Array(subscription.also.enumerated()), id: \.offset) { _, second in
            Text(verbatim: second.secondText)
                .textStyle(Tokens.Text.caption)
                .accessibilityIdentifier("secondSubscription")
            if second == .stripe, model.canCancelSecond {
                Button("Cancel the web subscription", role: .destructive) { Task { await model.cancel() } }
                    .disabled(model.busy)
            } else if second == .appStore || second == .play {
                storeManagement(second)
            }
        }
    }

    /// While one applies (D72 §1): one tap for Stripe and Google Play,
    /// Apple's sheet on iOS and Apple's page on the Mac for the App Store.
    @ViewBuilder
    private var refundRows: some View {
        if model.canRefund, let refundableUntil = subscription.refundableUntil {
            Button("Request a refund") { Task { await model.refund() } }
                .disabled(model.busy)
                .accessibilityIdentifier("requestRefund")
            Text("Ends your plan now and pays back the days you haven't used. Until \(refundableUntil.formatted(date: .abbreviated, time: .omitted)).")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
        } else if model.canRequestStoreRefund {
            Button("Request a refund") { Task { await model.requestStoreRefund() } }
                .accessibilityIdentifier("requestRefund")
            Group {
                if model.refundRequested {
                    Text("Apple has your request. If Apple refunds it, your plan ends and updates here.")
                } else {
                    Text("Apple decides on refunds for App Store purchases.")
                }
            }
            .textStyle(Tokens.Text.caption)
            .foregroundStyle(Tokens.Color.textMuted.color)
        } else if let url = model.storeRefundURL {
            Text("To ask for a refund, use Apple's Report a Problem page.")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
            Button("Open Report a Problem") { openURL(url) }
                .accessibilityIdentifier("requestRefund")
        }
    }

    /// iOS opens the App Store's page over the app; the Mac opens Apple's or
    /// Google's page in the browser. Google Play on iOS gets the words only.
    @ViewBuilder
    private func storeManagement(_ provider: BillingProvider) -> some View {
        if provider == .appStore, model.canManageInStore {
            Button("Manage subscription") { Task { await model.manageInStore() } }
                .accessibilityIdentifier("manageSubscription")
        } else if let url = model.manageURL(for: provider) {
            Button(provider == .appStore ? "Manage in the App Store" : "Manage in Google Play") { openURL(url) }
                .accessibilityIdentifier("manageSubscription")
        } else if provider == .play {
            Text("Cancel it in Google Play on your Android device.")
                .textStyle(Tokens.Text.caption)
                .foregroundStyle(Tokens.Color.textMuted.color)
        }
    }
}
#endif
