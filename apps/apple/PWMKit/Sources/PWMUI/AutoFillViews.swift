// The AutoFill extension's screen (D47 §4): the matching logins, a search
// over the rest, passkeys, and "Save passkey". Nothing fills without a tap,
// and nothing moves under the finger while the list loads.
#if canImport(SwiftUI)
import PWMDesign
import PWMKit
import SwiftUI

public struct AutoFillView: View {
    @Bindable var model: AutoFillModel

    public init(model: AutoFillModel) {
        self.model = model
    }

    public var body: some View {
        NavigationStack {
            content
                .navigationTitle("PWM")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { Task { await model.cancel() } }
                            .accessibilityIdentifier("autofillCancel")
                    }
                }
        }
        .tint(Tokens.Color.accent.color)
        // Buttons are pills (D1), as in the app.
        .buttonBorderShape(.capsule)
        // Manrope for any text that sets no style of its own (D1), as in the app.
        .font(.app(.body))
        .task { await model.start() }
    }

    @ViewBuilder private var content: some View {
        switch model.screen {
        case .starting:
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case let .blocked(line):
            Text(verbatim: line)
                .textStyle(Tokens.Text.body)
                .multilineTextAlignment(.center)
                .padding(Tokens.Space.s5)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("autofillBlocked")
        case .logins:
            logins
        case .passkeys:
            passkeys
        case .newPasskey:
            newPasskey
        }
    }

    private var logins: some View {
        List {
            if !model.matches.isEmpty && model.query.isEmpty {
                Section {
                    ForEach(model.matches) { choice in row(choice) }
                } header: {
                    SectionHeader("For this site")
                }
            }
            Section(model.query.isEmpty ? "Other logins" : "Results") {
                ForEach(model.others) { choice in row(choice) }
            }
            ErrorText(text: model.error)
        }
        .searchable(text: $model.query, prompt: "Search logins")
    }

    private func row(_ choice: FillChoice) -> some View {
        Button {
            Task { await model.fill(choice) }
        } label: {
            VStack(alignment: .leading, spacing: Tokens.Space.s1) {
                Text(verbatim: choice.title).textStyle(Tokens.Text.body)
                if !choice.username.isEmpty {
                    Text(verbatim: choice.username)
                        .textStyle(Tokens.Text.caption)
                        .foregroundStyle(Tokens.Color.textMuted.color)
                }
            }
        }
        .accessibilityIdentifier("fill-\(choice.itemId)")
    }

    private var passkeys: some View {
        List {
            if model.passkeys.isEmpty {
                Text("No passkey for this site in your vault.")
                    .foregroundStyle(Tokens.Color.textMuted.color)
            }
            ForEach(model.passkeys) { choice in
                Button {
                    Task { await model.choose(choice) }
                } label: {
                    Label { Text(verbatim: choice.userName) } icon: { Image(systemName: "person.badge.key") }
                }
            }
            ErrorText(text: model.error)
        }
    }

    private var newPasskey: some View {
        VStack(spacing: Tokens.Space.s4) {
            if let site = model.newPasskeySite {
                Text("Save a passkey for \(Text(verbatim: site.rpId))?")
                    .textStyle(Tokens.Text.title)
                    .multilineTextAlignment(.center)
                Text(verbatim: site.userName)
                    .foregroundStyle(Tokens.Color.textMuted.color)
            }
            Button("Save passkey") { Task { await model.saveNewPasskey() } }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("savePasskey")
            ErrorText(text: model.error)
        }
        .padding(Tokens.Space.s5)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
#endif
