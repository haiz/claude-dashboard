import Foundation
import Darwin

/// A raw-TCP HTTP server for the helper's transport tests.
///
/// Raw sockets rather than a higher-level API because the failure modes are the
/// point: closing a connection mid-request, staying silent past the client's
/// 15-second timeout, and returning a body that is not valid UTF-8 are all
/// things the transport must handle, and all things a framework would smooth
/// over.
///
/// It also **records** every request. That is what `MockURLProtocol` can never
/// do: it observes a URLRequest, not the bytes URLSession puts on the socket.
final class LoopbackServer {

    struct RecordedRequest {
        let method: String
        let path: String
        /// Header names lowercased; HTTP header names are case-insensitive.
        let headers: [String: String]
    }

    enum Response {
        case reply(status: Int, headers: [String: String], body: Data)
        case closeImmediately
        case staySilent

        static func json(_ text: String, status: Int = 200) -> Response {
            .reply(status: status, headers: [:], body: Data(text.utf8))
        }
    }

    /// Matches `/api/organizations/<anything>/usage`.
    static let usageWildcard = "/api/organizations/*/usage"

    private let lock = NSLock()
    private var routes: [String: Response] = [:]
    private var records: [RecordedRequest] = []
    private var listenFD: Int32 = -1
    private var heldConnections: [Int32] = []
    private var running = false
    private let queue = DispatchQueue(label: "loopback-server", qos: .userInitiated)

    private(set) var port: UInt16 = 0

    var origin: String { "http://127.0.0.1:\(port)" }

    var recorded: [RecordedRequest] {
        lock.lock(); defer { lock.unlock() }
        return records
    }

    func respond(path: String, with response: Response) {
        lock.lock(); defer { lock.unlock() }
        routes[path] = response
    }

    func start() {
        listenFD = socket(AF_INET, SOCK_STREAM, 0)
        precondition(listenFD >= 0, "socket() failed")

        var yes: Int32 = 1
        setsockopt(listenFD, SOL_SOCKET, SO_REUSEADDR, &yes, socklen_t(MemoryLayout<Int32>.size))

        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = 0                      // kernel picks a free port
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(listenFD, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        precondition(bound == 0, "bind() failed")
        precondition(listen(listenFD, 8) == 0, "listen() failed")

        var actual = sockaddr_in()
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        _ = withUnsafeMutablePointer(to: &actual) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                getsockname(listenFD, $0, &length)
            }
        }
        port = UInt16(bigEndian: actual.sin_port)

        running = true
        queue.async { [weak self] in self?.acceptLoop() }
    }

    func stop() {
        lock.lock()
        running = false
        let held = heldConnections
        heldConnections = []
        lock.unlock()

        for fd in held { close(fd) }
        if listenFD >= 0 {
            // shutdown() before close(): closing a descriptor another thread is
            // blocked in accept() on does not reliably wake it on Darwin, and a
            // server left blocked there hangs the next test's tearDown.
            shutdown(listenFD, SHUT_RDWR)
            close(listenFD)
            listenFD = -1
        }
    }

    // MARK: - Private

    private func acceptLoop() {
        while true {
            lock.lock(); let isRunning = running; lock.unlock()
            guard isRunning else { return }

            let fd = accept(listenFD, nil, nil)
            guard fd >= 0 else { return }   // listen socket closed by stop()
            handle(connection: fd)
        }
    }

    private func handle(connection fd: Int32) {
        guard let request = readRequest(fd) else { close(fd); return }

        lock.lock()
        records.append(request)
        let response = routes[request.path] ?? routes[Self.usageWildcard].flatMap {
            Self.matchesUsageWildcard(request.path) ? $0 : nil
        }
        lock.unlock()

        guard let response else {
            // A path nobody scripted is a test-authoring error. Answer 404 so
            // the test fails on `HTTP 404` instead of hanging.
            write(fd, status: 404, headers: [:], body: Data())
            close(fd)
            return
        }

        switch response {
        case .reply(let status, let headers, let body):
            write(fd, status: status, headers: headers, body: body)
            close(fd)
        case .closeImmediately:
            close(fd)
        case .staySilent:
            // Hold the connection open: closing it would surface as a network
            // error, not as the client's 15-second timeout.
            lock.lock(); heldConnections.append(fd); lock.unlock()
        }
    }

    static func matchesUsageWildcard(_ path: String) -> Bool {
        path.hasPrefix("/api/organizations/") && path.hasSuffix("/usage")
    }

    private func readRequest(_ fd: Int32) -> RecordedRequest? {
        var raw = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while raw.range(of: Data("\r\n\r\n".utf8)) == nil {
            let n = Darwin.read(fd, &buffer, buffer.count)
            if n <= 0 { return nil }
            raw.append(contentsOf: buffer[0..<n])
        }
        guard let text = String(data: raw, encoding: .utf8) else { return nil }
        let lines = text.components(separatedBy: "\r\n")
        let requestLine = lines.first?.components(separatedBy: " ") ?? []
        guard requestLine.count >= 2 else { return nil }

        var headers: [String: String] = [:]
        for line in lines.dropFirst() {
            if line.isEmpty { break }
            guard let colon = line.firstIndex(of: ":") else { continue }
            let name = line[line.startIndex..<colon].lowercased()
            let value = line[line.index(after: colon)...]
                .trimmingCharacters(in: .whitespaces)
            headers[name] = value
        }
        return RecordedRequest(method: requestLine[0], path: requestLine[1], headers: headers)
    }

    private func write(_ fd: Int32, status: Int, headers: [String: String], body: Data) {
        var head = "HTTP/1.1 \(status) X\r\nContent-Length: \(body.count)\r\nConnection: close\r\n"
        for (name, value) in headers { head += "\(name): \(value)\r\n" }
        head += "\r\n"
        var out = Data(head.utf8)
        out.append(body)
        out.withUnsafeBytes { raw in
            var sent = 0
            while sent < raw.count {
                let n = Darwin.write(fd, raw.baseAddress!.advanced(by: sent), raw.count - sent)
                if n <= 0 { return }
                sent += n
            }
        }
    }
}
