import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// A request the core asked for, made absolute. It may carry a token in its
/// headers: never log it.
public struct HTTPRequest: Sendable {
    public let method: String
    public let url: URL
    public let headers: [(name: String, value: String)]
    public let body: Data?
}

public struct HTTPResponse: Sendable {
    public let status: UInt16
    public let headers: [(name: String, value: String)]
    public let body: Data

    public init(status: UInt16, headers: [(name: String, value: String)] = [], body: Data = Data()) {
        self.status = status
        self.headers = headers
        self.body = body
    }
}

/// Sends the core's requests (D31 §2). Throw for a transport failure (no
/// connection, TLS, timeout); every HTTP status, errors included, is a
/// response for the core to read.
public protocol HTTPTransport: Sendable {
    func send(_ request: HTTPRequest) async throws -> HTTPResponse
}

/// `URLSession` with an ephemeral configuration: no cookies and no cache on
/// disk. TLS is the system's.
public final class URLSessionTransport: HTTPTransport, @unchecked Sendable {
    private let session: URLSession

    public init(timeout: TimeInterval = 30) {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = timeout
        session = URLSession(configuration: configuration)
    }

    public func send(_ request: HTTPRequest) async throws -> HTTPResponse {
        var urlRequest = URLRequest(url: request.url)
        urlRequest.httpMethod = request.method
        urlRequest.httpBody = request.body
        for header in request.headers {
            urlRequest.setValue(header.value, forHTTPHeaderField: header.name)
        }
        let request = urlRequest
        return try await withCheckedThrowingContinuation { continuation in
            session.dataTask(with: request) { data, response, error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }
                guard let response = response as? HTTPURLResponse,
                      let status = UInt16(exactly: response.statusCode)
                else {
                    continuation.resume(throwing: URLError(.badServerResponse))
                    return
                }
                let headers = response.allHeaderFields.compactMap { key, value -> (name: String, value: String)? in
                    guard let name = key as? String, let value = value as? String else { return nil }
                    return (name: name.lowercased(), value: value)
                }
                continuation.resume(returning: HTTPResponse(status: status, headers: headers, body: data ?? Data()))
            }.resume()
        }
    }
}
