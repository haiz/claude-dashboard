import XCTest
import Darwin

/// The harness testing itself. `LoopbackServer` is shared by every transport
/// test in this bundle, so a lifecycle bug in it does not fail loudly — it
/// serves one test's scripted body to a different test.
final class LoopbackServerTests: XCTestCase {

    /// `stop()` must not return while the accept thread is still parked on the
    /// listening descriptor.
    ///
    /// `close()` is what wakes a parked `accept()` on Darwin (it returns
    /// ECONNABORTED; `shutdown()` on a *listening* socket is inert — -1 with
    /// ENOTCONN). That is a use-after-close by construction: between `close()`
    /// returning and the kernel unwinding `accept()`, the fd number is free,
    /// and the next `LoopbackServer`'s `socket()` can reclaim it. The stale
    /// thread then accepts the NEW server's connection and answers it from the
    /// OLD instance's routes — `[weak self]` does not help, because
    /// `self?.acceptLoop()` retains self for the whole call, so the old server
    /// outlives its owner going nil.
    ///
    /// The symptom is a test served the previous test's body while its own
    /// `recorded` stays empty. This test asserts the invariant rather than the
    /// symptom, because the symptom needs the stale thread caught in a
    /// nanosecond-wide window and so cannot be made to fail on demand. The
    /// invariant can: measured on this machine, an unjoined `stop()` returned
    /// with the thread still live in 117 of 200 cycles.
    func testStopDoesNotReturnWhileTheAcceptThreadIsStillLive() {
        var stillLive = 0

        for _ in 0..<200 {
            let server = LoopbackServer()
            server.start()
            server.stop()
            if server.acceptLoopIsLive { stillLive += 1 }
        }

        XCTAssertEqual(stillLive, 0,
                       "stop() returned while the accept thread was still parked on a closed descriptor")
    }

    /// The end-to-end companion: consecutive servers must each serve their own
    /// routes and record their own requests.
    ///
    /// Honest about what this is worth — it did NOT reproduce the steal on the
    /// unfixed code (0 of 200 back-to-back cycles), for the window reason
    /// above. It is kept as a check on the property the join protects, not as
    /// the test that discriminates; that one is `testStopDoesNotReturn…`.
    func testConsecutiveServersEachServeTheirOwnRoutes() {
        var servedByAStaleServer = 0
        var missingFromRecorded = 0

        for iteration in 0..<200 {
            let server = LoopbackServer()
            server.start()
            let body = "iteration-\(iteration)"
            server.respond(path: "/probe", with: .json(body))

            let received = Self.get(path: "/probe", port: server.port)
            if received != body { servedByAStaleServer += 1 }
            if server.recorded.count != 1 { missingFromRecorded += 1 }

            server.stop()
        }

        XCTAssertEqual(servedByAStaleServer, 0,
                       "a previous instance's accept thread answered this server's connection")
        XCTAssertEqual(missingFromRecorded, 0,
                       "the request was recorded on some other instance")
    }

    /// A route nobody scripted answers 404 rather than hanging, so a
    /// mis-authored transport test fails on `HTTP 404` instead of timing out.
    func testUnscriptedPathAnswers404() {
        let server = LoopbackServer()
        server.start()
        defer { server.stop() }

        let response = Self.getRaw(path: "/nothing-scripted", port: server.port)

        XCTAssertTrue(response?.hasPrefix("HTTP/1.1 404 ") == true,
                      "got: \(response ?? "<nothing>")")
        XCTAssertEqual(server.recorded.first?.path, "/nothing-scripted")
    }

    // MARK: - A minimal blocking HTTP client

    /// Returns the response body, or nil if the exchange failed. Both socket
    /// timeouts are set: a harness test that hangs the suite would be worse
    /// than the race it is guarding.
    private static func get(path: String, port: UInt16) -> String? {
        guard let raw = getRaw(path: path, port: port),
              let range = raw.range(of: "\r\n\r\n") else { return nil }
        return String(raw[range.upperBound...])
    }

    /// The whole response, headers included.
    private static func getRaw(path: String, port: UInt16) -> String? {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { return nil }
        defer { close(fd) }

        var timeout = timeval(tv_sec: 2, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = port.bigEndian
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let connected = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard connected == 0 else { return nil }

        let request = "GET \(path) HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        let sent = Data(request.utf8).withUnsafeBytes { raw in
            Darwin.write(fd, raw.baseAddress!, raw.count)
        }
        guard sent == request.utf8.count else { return nil }

        var response = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while true {
            let n = Darwin.read(fd, &buffer, buffer.count)
            if n <= 0 { break }
            response.append(contentsOf: buffer[0..<n])
        }
        return String(data: response, encoding: .utf8)
    }
}
