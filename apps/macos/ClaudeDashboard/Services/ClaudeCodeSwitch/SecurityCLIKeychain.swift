import Foundation

/// Generic-password items in the login Keychain, addressed by (service, account).
protocol KeychainStoring {
    /// nil when the item does not exist.
    func read(service: String, account: String) throws -> Data?
    func write(_ data: Data, service: String, account: String) throws
}

enum KeychainError: Error, Equatable {
    case commandFailed(status: Int32)
    /// The write reported success but reading the item back returned other bytes.
    case writeNotPersisted
}

/// Keychain access through `/usr/bin/security`, the tool Claude Code itself uses.
///
/// Claude Code's entry trusts `security` in its access list, so going through it reads
/// and updates `Claude Code-credentials` without an access prompt; a `SecItem` call from
/// this app would prompt. Writes go to `security -i` on stdin with hex data (`-X`), so no
/// secret ever appears in a process argument list.
///
///     let kc = SecurityCLIKeychain()
///     try kc.write(data, service: "ClaudeDashboard.cc-vault", account: id.uuidString)
///     let back = try kc.read(service: "ClaudeDashboard.cc-vault", account: id.uuidString)
struct SecurityCLIKeychain: KeychainStoring {
    /// Runs `/usr/bin/security` with `arguments`, feeding `stdin`; returns status and stdout.
    typealias Runner = (_ arguments: [String], _ stdin: Data?) throws -> (status: Int32, stdout: Data)

    /// `security` exits with this when the item does not exist.
    static let itemNotFoundStatus: Int32 = 44

    private let run: Runner

    init(run: @escaping Runner = SecurityCLIKeychain.runSecurity) {
        self.run = run
    }

    func read(service: String, account: String) throws -> Data? {
        let result = try run(["find-generic-password", "-a", account, "-s", service, "-w"], nil)
        if result.status == Self.itemNotFoundStatus { return nil }
        guard result.status == 0 else { throw KeychainError.commandFailed(status: result.status) }
        return Self.decodePasswordOutput(result.stdout)
    }

    func write(_ data: Data, service: String, account: String) throws {
        let line = "add-generic-password -U -a \(Self.quote(account)) -s \(Self.quote(service)) -X \(Self.hex(data))\n"
        let result = try run(["-i"], Data(line.utf8))
        guard result.status == 0 else { throw KeychainError.commandFailed(status: result.status) }
        // `security -i` can report success for a command it rejected; read back to be sure.
        guard try read(service: service, account: account) == data else {
            throw KeychainError.writeNotPersisted
        }
    }

    /// `security -w` prints the password and a newline, or, when the bytes are not
    /// printable ASCII, the bytes as hex. JSON always contains `{`, so an all-hex line
    /// is never a plain JSON password.
    static func decodePasswordOutput(_ out: Data) -> Data {
        var text = String(decoding: out, as: UTF8.self)
        if text.hasSuffix("\n") { text.removeLast() }
        if !text.isEmpty, text.count % 2 == 0, text.allSatisfy(\.isHexDigit),
           let bytes = unhex(text) {
            return bytes
        }
        return Data(text.utf8)
    }

    static func runSecurity(_ arguments: [String], _ stdin: Data?) throws -> (status: Int32, stdout: Data) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = arguments
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        let input = Pipe()
        process.standardInput = input
        try process.run()
        if let stdin { input.fileHandleForWriting.write(stdin) }
        try input.fileHandleForWriting.close()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, data)
    }

    private static func quote(_ value: String) -> String {
        "\"" + value.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"") + "\""
    }

    private static func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    private static func unhex(_ text: String) -> Data? {
        var bytes = Data(capacity: text.count / 2)
        var index = text.startIndex
        while index < text.endIndex {
            let next = text.index(index, offsetBy: 2)
            guard let byte = UInt8(text[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        return bytes
    }
}
