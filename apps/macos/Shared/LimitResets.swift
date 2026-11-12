import Foundation

/// Free "limit resets" granted by claude.ai (a Full reset and Fable resets,
/// each with an expiry), read from the `/usage` payload.
///
/// The payload carries them under an unreleased internal field name, called
/// "the limit-resets field" everywhere in this repo. The name is never stored
/// raw (`scripts/test-no-codename.sh` enforces it): it is kept base64-encoded
/// and decoded at runtime by `fieldName`. The `/usage` request must carry
/// `?<fieldName>=1` for the field to be filled. The app only displays resets;
/// it never calls the reset endpoint.
///
/// Rules: `contract/README.md` "Limit resets", cases in
/// `contract/cases/limit-resets.json`. Mirrors `core::limit_resets` (Rust).
///
/// Example:
///
///     let grants = LimitResets.parseGrants(object[LimitResets.fieldName])
///     let summary = LimitResets.summarize(grants, now: Date())
enum LimitResets {
    static let encodedFieldName = "Y2VkYXJfZW1iZXI="

    /// A reset expiring in less than this many seconds is urgent (7 days).
    static let urgentSeconds: TimeInterval = 604_800

    /// The decoded name of the limit-resets field (query parameter and JSON key).
    static var fieldName: String {
        guard let data = Data(base64Encoded: encodedFieldName),
              let name = String(data: data, encoding: .utf8) else {
            preconditionFailure("encodedFieldName is not valid base64 UTF-8")
        }
        return name
    }

    /// Parses the limit-resets object (from `JSONSerialization`, or nil/NSNull).
    /// Time-independent: keeps every well-formed, unpaused grant with
    /// `resets_left >= 1`. Anything malformed degrades to fewer grants.
    static func parseGrants(_ object: Any?) -> [ResetGrant] {
        guard let obj = object as? [String: Any],
              boolValue(obj["eligible"]) == true,
              let grants = obj["grants"] as? [Any] else { return [] }
        return grants.compactMap(parseGrant)
    }

    /// Drops grants that ended at or before `now`, then sums counts (saturating)
    /// and takes the earliest expiry per kind.
    static func summarize(_ grants: [ResetGrant], now: Date) -> LimitResetsSummary {
        func of(_ kind: ResetKind) -> ResetSummary? {
            let live = grants.filter { $0.kind == kind && $0.endsAt > now }
            guard let expiresAt = live.map(\.endsAt).min() else { return nil }
            let count = live.reduce(0) { sum, g in
                let (s, overflow) = sum.addingReportingOverflow(g.resetsLeft)
                return overflow ? Int.max : s
            }
            return ResetSummary(count: count, expiresAt: expiresAt,
                                urgent: expiresAt.timeIntervalSince(now) < urgentSeconds)
        }
        return LimitResetsSummary(full: of(.full), fable: of(.fable))
    }

    // MARK: - Private

    private static func parseGrant(_ raw: Any) -> ResetGrant? {
        guard let g = raw as? [String: Any],
              let resetsLeft = integralCount(g["resets_left"]),
              boolValue(g["paused"]) != true,
              let endsText = g["ends_at"] as? String,
              let endsAt = parseDate(endsText) else { return nil }
        let kind: ResetKind
        if boolValue(g["gives_back_weekly"]) == true {
            kind = .fable
        } else if let clears = g["clears"] as? [Any],
                  clears.contains(where: { ($0 as? String) == "five_hour" }),
                  clears.contains(where: { ($0 as? String) == "seven_day" }) {
            kind = .full
        } else {
            return nil
        }
        return ResetGrant(kind: kind, resetsLeft: resetsLeft, endsAt: endsAt)
    }

    /// True only for a genuine JSON boolean (an NSNumber of the CFBoolean type).
    private static func boolValue(_ v: Any?) -> Bool? {
        guard let n = v as? NSNumber, CFGetTypeID(n) == CFBooleanGetTypeID() else { return nil }
        return n.boolValue
    }

    /// A JSON number holding an integer >= 1 (`1` and `1.0` count); booleans,
    /// strings, non-integral numbers and values above u32 max (as Rust and JS) give nil.
    private static func integralCount(_ v: Any?) -> Int? {
        guard let n = v as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID() else { return nil }
        let d = n.doubleValue
        guard d.rounded(.towardZero) == d, d >= 1, d <= 4294967295 else { return nil }
        return n.intValue
    }

    /// ISO8601 with or without fractional seconds, truncated to whole seconds
    /// like the Rust parser.
    private static func parseDate(_ text: String) -> Date? {
        let withFraction = ISO8601DateFormatter()
        withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let basic = ISO8601DateFormatter()
        basic.formatOptions = [.withInternetDateTime]
        guard let date = withFraction.date(from: text) ?? basic.date(from: text) else { return nil }
        return Date(timeIntervalSince1970: date.timeIntervalSince1970.rounded(.down))
    }
}

enum ResetKind: Equatable { case full, fable }

/// One usable grant.
struct ResetGrant: Equatable {
    let kind: ResetKind
    let resetsLeft: Int
    let endsAt: Date
}

struct ResetSummary: Equatable {
    let count: Int
    let expiresAt: Date
    let urgent: Bool
}

struct LimitResetsSummary: Equatable {
    let full: ResetSummary?
    let fable: ResetSummary?
}
