import Foundation

/// What can go wrong asking a server something.
enum HTTPError: Error, Equatable, LocalizedError {
    case invalidURL(String)
    case unsupportedScheme
    case request(String)
    case status(Int)
    case tooLarge(limit: Int)

    var errorDescription: String? {
        switch self {
        case let .invalidURL(address): "\"\(address)\" is not a web address"
        case .unsupportedScheme: "only http and https addresses can be used"
        case let .request(message): "could not reach the server: \(message)"
        case let .status(code): "the server answered with error \(code)"
        case let .tooLarge(limit): "the answer was larger than \(limit) bytes"
        }
    }
}

/// The answer to a request: its status and body, whatever the status.
struct HTTPAnswer: Sendable {
    var status: Int
    var body: Data
    var ok: Bool { (200 ..< 300).contains(status) }
}

/// The web client every network feature shares: Haven's user agent, its
/// time limits, and answers that may not grow past a size the caller sets
/// (so a wrong address can never fill the phone's memory).
final class HTTP: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    static let shared = HTTP()

    /// Time allowed for a whole request unless the caller says otherwise.
    static let defaultTimeout: TimeInterval = 30

    static var userAgent: String { "Haven/\(Build.version)" }

    private var session: URLSession!
    private let lock = NSLock()
    private var running: [Int: Collector] = [:]

    /// `protocolClasses` lets tests answer requests without a network.
    init(protocolClasses: [AnyClass]? = nil) {
        super.init()
        let config = URLSessionConfiguration.default
        if let protocolClasses { config.protocolClasses = protocolClasses }
        config.httpAdditionalHeaders = ["User-Agent": Self.userAgent]
        // Time allowed between two pieces of an answer; the whole request's
        // limit is set on each task.
        config.timeoutIntervalForRequest = Self.defaultTimeout
        config.timeoutIntervalForResource = 24 * 60 * 60
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        session = URLSession(configuration: config, delegate: self, delegateQueue: nil)
    }

    /// Parses an address and accepts only http and https.
    static func webURL(_ address: String) throws -> URL {
        let trimmed = address.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: trimmed, encodingInvalidCharacters: false),
              let scheme = url.scheme?.lowercased()
        else {
            throw HTTPError.invalidURL(trimmed)
        }
        guard scheme == "http" || scheme == "https" else { throw HTTPError.unsupportedScheme }
        guard let host = url.host(), !host.isEmpty else { throw HTTPError.invalidURL(trimmed) }
        return url
    }

    /// Fetches `address`, giving up once the body passes `limit` bytes.
    /// Anything but a 2xx answer is an error.
    func getBytes(_ address: String, limit: Int, headers: [(String, String)] = [],
                  timeout: TimeInterval = HTTP.defaultTimeout) async throws -> Data {
        let answer = try await send(address, headers: headers, limit: limit, timeout: timeout)
        guard answer.ok else { throw HTTPError.status(answer.status) }
        return answer.body
    }

    /// Sends a request and reads the answer, giving up once it passes
    /// `limit` bytes. A body without a method is POSTed as `contentType`.
    /// Returns the status with the body whatever the status is, so callers
    /// can read an error's details.
    func send(_ address: String, method: String? = nil, headers: [(String, String)] = [],
              body: Data? = nil, contentType: String = "application/json", limit: Int,
              timeout: TimeInterval = HTTP.defaultTimeout) async throws -> HTTPAnswer {
        let url = try Self.webURL(address)
        var request = URLRequest(url: url, timeoutInterval: timeout)
        request.httpMethod = method ?? (body == nil ? "GET" : "POST")
        if let body {
            request.httpBody = body
            request.setValue(contentType, forHTTPHeaderField: "Content-Type")
        }
        for (name, value) in headers {
            request.setValue(value, forHTTPHeaderField: name)
        }
        return try await perform(request, limit: limit, timeout: timeout)
    }

    // MARK: Running requests

    private final class Collector {
        let limit: Int
        var status = 0
        var body = Data()
        var continuation: CheckedContinuation<HTTPAnswer, Error>?
        var timer: DispatchWorkItem?
        init(limit: Int) { self.limit = limit }
    }

    private func perform(_ request: URLRequest, limit: Int, timeout: TimeInterval) async throws -> HTTPAnswer {
        let task = session.dataTask(with: request)
        let collector = Collector(limit: limit)
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                lock.lock()
                collector.continuation = continuation
                running[task.taskIdentifier] = collector
                // The whole request's limit (URLSession only limits the gaps).
                let timer = DispatchWorkItem { [weak self] in
                    self?.finish(task.taskIdentifier, with: .failure(HTTPError.request("the server took too long to answer")))
                    task.cancel()
                }
                collector.timer = timer
                lock.unlock()
                DispatchQueue.global().asyncAfter(deadline: .now() + timeout, execute: timer)
                task.resume()
            }
        } onCancel: {
            task.cancel()
        }
    }

    private func finish(_ id: Int, with result: Result<HTTPAnswer, Error>) {
        lock.lock()
        let collector = running.removeValue(forKey: id)
        let continuation = collector?.continuation
        collector?.continuation = nil
        collector?.timer?.cancel()
        lock.unlock()
        continuation?.resume(with: result)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                    completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        lock.lock()
        let collector = running[dataTask.taskIdentifier]
        collector?.status = (response as? HTTPURLResponse)?.statusCode ?? 0
        lock.unlock()
        // A size the server announces past the limit: stop before reading.
        if let collector, response.expectedContentLength > Int64(collector.limit) {
            finish(dataTask.taskIdentifier, with: .failure(HTTPError.tooLarge(limit: collector.limit)))
            completionHandler(.cancel)
            return
        }
        completionHandler(.allow)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        lock.lock()
        guard let collector = running[dataTask.taskIdentifier] else {
            lock.unlock()
            return
        }
        let tooLarge = collector.body.count + data.count > collector.limit
        if !tooLarge { collector.body.append(data) }
        lock.unlock()
        if tooLarge {
            finish(dataTask.taskIdentifier, with: .failure(HTTPError.tooLarge(limit: collector.limit)))
            dataTask.cancel()
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        lock.lock()
        let collector = running[task.taskIdentifier]
        lock.unlock()
        guard let collector else { return }
        if let error {
            let cancelled = (error as? URLError)?.code == .cancelled
            finish(task.taskIdentifier, with: .failure(cancelled ? CancellationError() : HTTPError.request(Self.words(error))))
        } else {
            finish(task.taskIdentifier, with: .success(HTTPAnswer(status: collector.status, body: collector.body)))
        }
    }

    /// An error in words, without the address (which may carry a key).
    static func words(_ error: Error) -> String {
        guard let error = error as? URLError else { return error.localizedDescription }
        switch error.code {
        case .timedOut: return "the server took too long to answer"
        case .cannotFindHost, .dnsLookupFailed: return "the server's name was not found"
        case .cannotConnectToHost: return "the server refused the connection"
        case .notConnectedToInternet: return "this device is not connected to the internet"
        case .networkConnectionLost: return "the connection was lost"
        case .secureConnectionFailed, .serverCertificateUntrusted, .serverCertificateHasBadDate,
             .serverCertificateNotYetValid, .serverCertificateHasUnknownRoot:
            return "the secure connection failed"
        default: return "network error \(error.code.rawValue)"
        }
    }
}
