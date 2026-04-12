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
    let sevenDaySonnet: UsageLimit?

    enum CodingKeys: String, CodingKey {
        case fiveHour = "five_hour"
        case sevenDay = "seven_day"
        case sevenDaySonnet = "seven_day_sonnet"
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
