import XCTest

@testable import Culler

final class FrontierClientTests: XCTestCase {
    private let config = FrontierConfig(
        baseURL: URL(string: "https://api.openai.com/v1")!,
        model: "luna-5.6",
        apiKey: "sk-test"
    )

    func testRequestCarriesModelAuthAndImages() throws {
        let request = try FrontierClient.buildRequest(
            config: config,
            prompt: "pick one",
            imageDataURLs: ["data:image/jpeg;base64,AAAA", "data:image/jpeg;base64,BBBB"]
        )

        XCTAssertEqual(request.url?.absoluteString, "https://api.openai.com/v1/chat/completions")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer sk-test")
        let body = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        XCTAssertEqual(body["model"] as? String, "luna-5.6")
        let messages = body["messages"] as! [[String: Any]]
        let content = messages[0]["content"] as! [[String: Any]]
        XCTAssertEqual(content.count, 3, "one text part plus two images")
        XCTAssertEqual(content[0]["type"] as? String, "text")
        XCTAssertEqual(content[1]["type"] as? String, "image_url")
    }

    private func reply(_ content: String) -> Data {
        let json = ["choices": [["message": ["content": content]]]]
        return try! JSONSerialization.data(withJSONObject: json)
    }

    func testParsesPlainAndFencedReplies() throws {
        let plain = try FrontierClient.parsePick(
            from: reply(#"{"keeper_index": 1, "reason": "sharper and eyes open"}"#),
            candidateCount: 3
        )
        XCTAssertEqual(plain, FrontierPick(keeperIndex: 1, reason: "sharper and eyes open"))

        let fenced = try FrontierClient.parsePick(
            from: reply("```json\n{\"keeper_index\": 0, \"reason\": \"best framing\"}\n```"),
            candidateCount: 2
        )
        XCTAssertEqual(fenced.keeperIndex, 0)
    }

    func testRejectsProseAndOutOfRangePicks() {
        XCTAssertThrowsError(
            try FrontierClient.parsePick(from: reply("The best photo is #2."), candidateCount: 3)
        )
        XCTAssertThrowsError(
            try FrontierClient.parsePick(
                from: reply(#"{"keeper_index": 9, "reason": "nope"}"#),
                candidateCount: 3
            )
        )
    }
}
