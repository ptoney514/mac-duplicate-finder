import Foundation

/// Frontier tiebreaks (§5.3, ADR-0006): an OpenAI-compatible chat
/// completions client. Sends the top thumbnails plus quality signals for
/// one cluster and expects a strict-JSON keeper pick with a one-line reason.
struct FrontierConfig: Sendable {
    var baseURL: URL
    var model: String
    var apiKey: String

    static let defaultBaseURL = "https://api.openai.com/v1"
    static let defaultModel = "luna-5.6"

    /// Reads settings; nil until an API key is stored (feature stays inert).
    static func current() -> FrontierConfig? {
        guard let key = KeychainStore.get(account: "frontier-api-key"), !key.isEmpty else {
            return nil
        }
        let defaults = UserDefaults.standard
        let base = defaults.string(forKey: "frontier.baseURL") ?? defaultBaseURL
        let model = defaults.string(forKey: "frontier.model") ?? defaultModel
        guard let url = URL(string: base) else { return nil }
        return FrontierConfig(baseURL: url, model: model, apiKey: key)
    }
}

struct FrontierPick: Equatable, Sendable {
    let keeperIndex: Int
    let reason: String
}

enum FrontierError: LocalizedError {
    case badStatus(Int, String)
    case malformed(String)

    var errorDescription: String? {
        switch self {
        case .badStatus(let code, let body):
            "Frontier API returned \(code): \(String(body.prefix(200)))"
        case .malformed(let detail):
            "Frontier reply was not the expected JSON: \(detail)"
        }
    }
}

enum FrontierClient {
    /// One candidate frame as sent to the model.
    struct Candidate {
        let label: String
        let qualityScore: Double?
        let thumbPath: String?
    }

    static func prompt(for candidates: [Candidate]) -> String {
        let signals = candidates.enumerated()
            .map { index, c in
                let quality = c.qualityScore.map { String(format: "%.3f", $0) } ?? "unknown"
                return "\(index): \(c.label) (engine quality \(quality))"
            }
            .joined(separator: "\n")
        return """
            These photos are near-identical frames from one burst, numbered in the \
            same order as the attached images (starting at 0):
            \(signals)
            Pick the single best frame to keep, weighing sharpness, open eyes, \
            expressions, and composition over the engine's quality score.
            Respond with ONLY this JSON, no prose: \
            {"keeper_index": <number>, "reason": "<one short sentence>"}
            """
    }

    static func buildRequest(
        config: FrontierConfig,
        prompt: String,
        imageDataURLs: [String]
    ) throws -> URLRequest {
        var content: [[String: Any]] = [["type": "text", "text": prompt]]
        for dataURL in imageDataURLs {
            content.append(["type": "image_url", "image_url": ["url": dataURL]])
        }
        let body: [String: Any] = [
            "model": config.model,
            "messages": [["role": "user", "content": content]],
        ]
        var request = URLRequest(
            url: config.baseURL.appendingPathComponent("chat/completions")
        )
        request.httpMethod = "POST"
        request.setValue("Bearer \(config.apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        return request
    }

    /// Parses a chat-completions reply; tolerates ```json fences.
    static func parsePick(from data: Data, candidateCount: Int) throws -> FrontierPick {
        struct Reply: Decodable {
            struct Choice: Decodable {
                struct Message: Decodable { let content: String }
                let message: Message
            }
            let choices: [Choice]
        }
        guard let reply = try? JSONDecoder().decode(Reply.self, from: data),
            var text = reply.choices.first?.message.content
        else {
            throw FrontierError.malformed("no choices[0].message.content")
        }
        text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("```") {
            text = text
                .replacingOccurrences(of: "```json", with: "")
                .replacingOccurrences(of: "```", with: "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }
        struct PickJSON: Decodable {
            let keeper_index: Int
            let reason: String
        }
        guard let pick = try? JSONDecoder().decode(PickJSON.self, from: Data(text.utf8)) else {
            throw FrontierError.malformed(String(text.prefix(120)))
        }
        guard (0..<candidateCount).contains(pick.keeper_index) else {
            throw FrontierError.malformed("keeper_index \(pick.keeper_index) out of range")
        }
        return FrontierPick(keeperIndex: pick.keeper_index, reason: pick.reason)
    }

    /// Asks the configured model to pick a keeper among `candidates`
    /// (thumbnails attached as base64 data URLs).
    static func askKeeper(
        config: FrontierConfig,
        candidates: [Candidate]
    ) async throws -> FrontierPick {
        let dataURLs: [String] = candidates.compactMap { candidate in
            guard let path = candidate.thumbPath,
                let data = try? Data(contentsOf: URL(fileURLWithPath: path))
            else { return nil }
            return "data:image/jpeg;base64,\(data.base64EncodedString())"
        }
        let request = try buildRequest(
            config: config,
            prompt: prompt(for: candidates),
            imageDataURLs: dataURLs
        )
        let (data, response) = try await URLSession.shared.data(for: request)
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw FrontierError.badStatus(
                http.statusCode,
                String(data: data, encoding: .utf8) ?? ""
            )
        }
        return try parsePick(from: data, candidateCount: candidates.count)
    }
}
