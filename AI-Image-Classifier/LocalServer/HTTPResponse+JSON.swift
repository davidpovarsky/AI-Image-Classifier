import FlyingFox
import Foundation

nonisolated enum JSONHTTPResponse {
    static func make<T: Encodable>(
        _ value: T,
        statusCode: HTTPStatusCode = .ok
    ) -> HTTPResponse {
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            let data = try encoder.encode(value)
            return HTTPResponse(
                statusCode: statusCode,
                headers: [
                    .contentType: "application/json; charset=utf-8",
                    .cacheControl: "no-store"
                ],
                body: data
            )
        } catch {
            let fallback = Data("{\"success\":false,\"error\":\"classification_failed\"}".utf8)
            return HTTPResponse(
                statusCode: .internalServerError,
                headers: [
                    .contentType: "application/json; charset=utf-8",
                    .cacheControl: "no-store"
                ],
                body: fallback
            )
        }
    }
}
