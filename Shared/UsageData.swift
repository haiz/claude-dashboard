import Foundation

struct UsageLimit: Codable, Equatable {
    let utilization: Double
    let resetsAt: Date?

    enum CodingKeys: String, CodingKey {
        case utilization
        case resetsAt = "resets_at"
    }
}

struct UsageData: Codable, Equatable {
    let fiveHour: UsageLimit
    let sevenDay: UsageLimit
    /// Weekly usage scoped to the Fable model. The usage endpoint has no
    /// `seven_day_fable` field — Fable is reported only inside the `limits`
    /// array as a `weekly_scoped` entry whose `scope.model.display_name` is
    /// "Fable". nil when the account has no Fable-scoped limit (the gauge stays
    /// hidden). Resets in lockstep with `sevenDay`.
    var fable: UsageLimit? = nil

    // Encoded shape only — `fable` is derived from `limits` on decode (see the
    // custom init below) and is never re-encoded.
    enum CodingKeys: String, CodingKey {
        case fiveHour = "five_hour"
        case sevenDay = "seven_day"
    }

    static func decode(from data: Data) throws -> UsageData {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            let dateString = try container.decode(String.self)

            let formatterWithFraction = ISO8601DateFormatter()
            formatterWithFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            if let date = formatterWithFraction.date(from: dateString) {
                return date
            }

            let formatterBasic = ISO8601DateFormatter()
            formatterBasic.formatOptions = [.withInternetDateTime]
            if let date = formatterBasic.date(from: dateString) {
                return date
            }

            throw DecodingError.dataCorruptedError(
                in: container,
                debugDescription: "Cannot decode date: \(dateString)"
            )
        }
        return try decoder.decode(UsageData.self, from: data)
    }
}

// MARK: - Custom decoding (the Fable window lives in the `limits` array)

extension UsageData {
    private enum RootKeys: String, CodingKey {
        case fiveHour = "five_hour"
        case sevenDay = "seven_day"
        case limits
    }

    /// Partial shape of a `limits[]` entry — only the fields needed to surface
    /// the Fable-scoped weekly window. All fields optional so unrelated entries
    /// (session, weekly_all, other scoped models) decode without throwing.
    private struct LimitEntry: Decodable {
        let percent: Double?
        let resetsAt: Date?
        let scope: Scope?

        struct Scope: Decodable {
            let model: Model?
            struct Model: Decodable {
                let displayName: String?
                enum CodingKeys: String, CodingKey { case displayName = "display_name" }
            }
        }

        enum CodingKeys: String, CodingKey {
            case percent, scope
            case resetsAt = "resets_at"
        }
    }

    // Defined in an extension so the compiler still synthesizes the memberwise
    // initializer (used by tests and by the delegation below).
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: RootKeys.self)
        let fiveHour = try container.decode(UsageLimit.self, forKey: .fiveHour)
        let sevenDay = try container.decode(UsageLimit.self, forKey: .sevenDay)

        // Fable is a model-scoped weekly limit nested in `limits`. A missing
        // array or no matching entry → nil.
        let limits = (try? container.decode([LimitEntry].self, forKey: .limits)) ?? []
        let fable = limits
            .first { $0.scope?.model?.displayName == "Fable" }
            .map { UsageLimit(utilization: $0.percent ?? 0, resetsAt: $0.resetsAt) }

        self.init(fiveHour: fiveHour, sevenDay: sevenDay, fable: fable)
    }
}
