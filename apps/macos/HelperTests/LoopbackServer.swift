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
    /// The connection `handle` is working on, published so `stop()` can
    /// `shutdown()` it. Set and cleared inside the same critical section that
    /// closes it, so `stop()` can never shut down a number that has already
    /// been closed and reused.
    private var activeConnection: Int32 = -1
    private var running = false
    private var acceptLoopLive = false
    private let queue = DispatchQueue(label: "loopback-server", qos: .userInitiated)
    private let acceptLoopExited = DispatchGroup()

    private(set) var port: UInt16 = 0

    /// True from before the accept thread starts until after it has returned.
    ///
    /// `stop()` must not return while this is true: see the comment on the
    /// join in `stop()` for what a surviving accept thread does to the *next*
    /// server. Exposed so a test can assert the invariant directly — the
    /// symptom it prevents is timing-dependent enough that a green suite
    /// proves nothing about it.
    var acceptLoopIsLive: Bool {
        lock.lock(); defer { lock.unlock() }
        return acceptLoopLive
    }

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
        acceptLoopLive = true
        let exited = acceptLoopExited          // captured strongly: stop() joins on it
        exited.enter()
        queue.async { [weak self] in
            defer {
                self?.markAcceptLoopFinished()
                exited.leave()
            }
            self?.acceptLoop()
        }
    }

    private func markAcceptLoopFinished() {
        lock.lock(); acceptLoopLive = false; lock.unlock()
    }

    func stop() {
        lock.lock()
        running = false
        let held = heldConnections
        heldConnections = []
        // shutdown(), not close(): the accept thread owns this descriptor and
        // closes it itself, so nothing here can free a number that thread is
        // about to touch. SHUT_RDWR makes its blocking read() return 0, which
        // is the only thing that would wake it out of readRequest.
        if activeConnection >= 0 { shutdown(activeConnection, SHUT_RDWR) }
        // Closed inside the critical section so that "listenFD is -1" and "the
        // descriptor is closed" are one step as far as the accept thread's
        // guarded read of listenFD is concerned.
        if listenFD >= 0 {
            // close(), NOT shutdown(): shutdown() on a *listening* socket
            // returns -1/ENOTCONN on Darwin and does nothing at all. close()
            // is what wakes a parked accept(), which returns -1/ECONNABORTED.
            // That is a use-after-close by construction -- it is only safe
            // because of the join below.
            close(listenFD)
            listenFD = -1
        }
        lock.unlock()

        for fd in held { close(fd) }

        // Join the accept thread. Without this, stop() returns while that
        // thread is still unwinding accept() on a descriptor whose number is
        // already free; the next server's socket() reclaims it, and the stale
        // thread then serves the NEW server's connections from the OLD
        // instance's routes, recording them where no test will look.
        // (`[weak self]` does not help: `self?.acceptLoop()` retains self for
        // the duration of the call, so the old server outlives its owner.)
        // Measured on this machine before the join: 117 of 200 stop() calls
        // returned with the thread still live.
        //
        // This cannot hang. The loop has exactly three blocking calls and each
        // has been woken above: accept() by the close; readRequest's read() by
        // the shutdown, which `handle` makes race-free by publishing the fd
        // and re-checking `running` in one critical section; and write(),
        // which cannot block on bodies that fit in a socket buffer and cannot
        // raise SIGPIPE now that accepted sockets carry SO_NOSIGPIPE. The
        // bound is a safety valve for a blocking call a later task might add,
        // not part of that argument -- a harness that hangs the whole suite
        // would be worse than the race it is fixing.
        if acceptLoopExited.wait(timeout: .now() + 5) == .timedOut {
            FileHandle.standardError.write(
                Data("LoopbackServer.stop: accept loop still running after 5s\n".utf8))
        }
    }

    // MARK: - Private

    private func acceptLoop() {
        while true {
            lock.lock(); let listener = running ? listenFD : -1; lock.unlock()
            guard listener >= 0 else { return }

            let connection = accept(listener, nil, nil)

            // Re-checked AFTER accept(), not only before it. stop() wakes a
            // parked accept() by closing the listening socket, so by the time
            // this returns the server may be gone -- and a thread that kept
            // going here is the stale thread the join in stop() exists to
            // prevent. No ECONNABORTED retry: on Darwin that errno *is* the
            // wake, so retrying would spin instead of exiting.
            lock.lock(); let stillRunning = running; lock.unlock()
            guard stillRunning else {
                if connection >= 0 { close(connection) }
                return
            }
            guard connection >= 0 else { return }

            handle(connection: connection)
        }
    }

    private func handle(connection fd: Int32) {
        // Without SO_NOSIGPIPE, writing to a peer that has already gone away
        // raises SIGPIPE, whose default disposition kills the whole test
        // process rather than just failing the write. Reachable from the
        // `.closeImmediately` and timeout cases.
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))

        // Publishing the fd and re-checking `running` in ONE critical section
        // is what makes the join hang-free. The two orders are the only two:
        // this section first, and stop() then sees `activeConnection` and
        // shuts the socket down, waking the read below; or stop()'s section
        // first, and `running` is false here so the connection is closed and
        // dropped. There is no interleaving that leaves a blocked read() with
        // nobody to wake it.
        lock.lock()
        guard running else { lock.unlock(); close(fd); return }
        activeConnection = fd
        lock.unlock()

        var handedOff = false     // true once heldConnections owns this fd
        defer {
            lock.lock()
            activeConnection = -1
            if !handedOff { close(fd) }
            lock.unlock()
        }

        guard let request = readRequest(fd) else { return }

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
            return
        }

        switch response {
        case .reply(let status, let headers, let body):
            write(fd, status: status, headers: headers, body: body)
        case .closeImmediately:
            break                 // the defer closes it
        case .staySilent:
            // Hold the connection open: closing it would surface as a network
            // error, not as the client's 15-second timeout. Only while the
            // server is up, though -- an fd parked here after stop() drained
            // heldConnections would never be closed by anyone.
            lock.lock()
            if running { heldConnections.append(fd); handedOff = true }
            lock.unlock()
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
