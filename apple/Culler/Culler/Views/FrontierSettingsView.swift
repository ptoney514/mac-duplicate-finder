import SwiftUI

/// Settings for the frontier tier (ADR-0006): any OpenAI-compatible
/// endpoint. Empty API key keeps the feature off.
struct FrontierSettingsView: View {
    @AppStorage("frontier.baseURL") private var baseURL = FrontierConfig.defaultBaseURL
    @AppStorage("frontier.model") private var model = FrontierConfig.defaultModel
    @State private var apiKey = KeychainStore.get(account: "frontier-api-key") ?? ""

    var body: some View {
        Form {
            Section("AI Tiebreaks") {
                TextField("Base URL", text: $baseURL, prompt: Text(FrontierConfig.defaultBaseURL))
                TextField("Model", text: $model, prompt: Text(FrontierConfig.defaultModel))
                SecureField("API Key", text: $apiKey)
                    .onChange(of: apiKey) {
                        KeychainStore.set(apiKey, account: "frontier-api-key")
                    }
                Text(
                    apiKey.isEmpty
                        ? "Enter an API key to enable the Ask AI button in the burst resolver."
                        : "Only the top 4 thumbnails of a cluster are sent, once per cluster."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 440)
        .padding()
    }
}
