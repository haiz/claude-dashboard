import Foundation
import CryptoKit

enum CryptoService {

    private static let salt = "com.claude-dashboard.v1".data(using: .utf8)!

    // MARK: - Public API

    static func encrypt(_ plaintext: String) -> String? {
        guard let key = deriveKey(),
              let data = plaintext.data(using: .utf8) else { return nil }

        let nonce = AES.GCM.Nonce()
        guard let sealed = try? AES.GCM.seal(data, using: key, nonce: nonce) else { return nil }
        return sealed.combined?.base64EncodedString()
    }

    static func decrypt(_ base64Ciphertext: String) -> String? {
        guard let key = deriveKey(),
              let combined = Data(base64Encoded: base64Ciphertext),
              let box = try? AES.GCM.SealedBox(combined: combined),
              let data = try? AES.GCM.open(box, using: key) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    // MARK: - Key Derivation

    private static var cachedKey: SymmetricKey?

    private static func deriveKey() -> SymmetricKey? {
        if let cached = cachedKey { return cached }
        guard let hwID = hardwareUUID()?.data(using: .utf8) else { return nil }
        let key = HKDF<SHA256>.deriveKey(inputKeyMaterial: SymmetricKey(data: hwID), salt: salt, outputByteCount: 32)
        cachedKey = key
        return key
    }

    private static func hardwareUUID() -> String? {
        let service = IOServiceMatching("IOPlatformExpertDevice")
        var iterator: io_iterator_t = 0
        guard IOServiceGetMatchingServices(kIOMainPortCompat, service, &iterator) == KERN_SUCCESS else { return nil }
        defer { IOObjectRelease(iterator) }

        let entry = IOIteratorNext(iterator)
        guard entry != 0 else { return nil }
        defer { IOObjectRelease(entry) }

        let key = "IOPlatformUUID" as CFString
        guard let uuid = IORegistryEntryCreateCFProperty(entry, key, kCFAllocatorDefault, 0)?
                .takeRetainedValue() as? String else { return nil }
        return uuid
    }
}

// IOKit main port compatibility across macOS versions
private let kIOMainPortCompat: mach_port_t = {
    if #available(macOS 12.0, *) {
        return kIOMainPortDefault
    } else {
        return 0 // kIOMasterPortDefault, deprecated
    }
}()
