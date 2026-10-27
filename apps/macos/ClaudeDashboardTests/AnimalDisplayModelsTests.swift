import XCTest
@testable import ClaudeDashboard

final class AnimalDisplayModelsTests: XCTestCase {
    private struct Fixture: Decodable {
        let phaseCases: [PhaseCase]
        let assetCases: [AssetCase]
        let activityCases: [ActivityCase]
        let eligibilityCases: [EligibilityCase]

        enum CodingKeys: String, CodingKey {
            case phaseCases = "phase_cases"
            case assetCases = "asset_cases"
            case activityCases = "activity_cases"
            case eligibilityCases = "eligibility_cases"
        }
    }

    private struct PhaseCase: Decodable {
        let name: String
        let phase: Double
        let expectContent: String
        let expectTransitionProgress: Double?
        let expectPercentageOpacity: Double
        let expectAnimalOpacity: Double
        let expectFrameIndex: Int?

        enum CodingKeys: String, CodingKey {
            case name, phase
            case expectContent = "expect_content"
            case expectTransitionProgress = "expect_transition_progress"
            case expectPercentageOpacity = "expect_percentage_opacity"
            case expectAnimalOpacity = "expect_animal_opacity"
            case expectFrameIndex = "expect_frame_index"
        }
    }

    private struct AssetCase: Decodable {
        let name: String
        let emoji: String
        let expectAssetID: String
        enum CodingKeys: String, CodingKey { case name, emoji; case expectAssetID = "expect_asset_id" }
    }

    private struct Operation: Decodable {
        let kind: String
        let accountID: String
        let utilization: Double?
        let resetsAt: Double?
        let at: Double
        enum CodingKeys: String, CodingKey {
            case kind, at, utilization
            case accountID = "account_id"
            case resetsAt = "resets_at"
        }
    }

    private struct ActivityCheck: Decodable {
        let accountID: String
        let at: Double
        let expectActive: Bool
        enum CodingKeys: String, CodingKey {
            case at
            case accountID = "account_id"
            case expectActive = "expect_active"
        }
    }

    private struct ActivityCase: Decodable {
        let name: String
        let operations: [Operation]
        let checks: [ActivityCheck]
    }

    private struct EligibilityCase: Decodable {
        let name: String
        let showBurnAnimals: Bool
        let isFiveHour: Bool
        let animal: String?
        let isActive: Bool
        let expectAssetID: String?
        enum CodingKeys: String, CodingKey {
            case name, animal
            case showBurnAnimals = "show_burn_animals"
            case isFiveHour = "is_five_hour"
            case isActive = "is_active"
            case expectAssetID = "expect_asset_id"
        }
    }

    func testContractCases() throws {
        let fixture = try JSONDecoder().decode(Fixture.self,
            from: ContractFixtures.data("cases/animal-display.json"))

        XCTAssertEqual(AnimalDisplayModels.cycleSeconds, 10)
        XCTAssertEqual(AnimalDisplayModels.activitySeconds, 600)
        XCTAssertEqual(AnimalDisplayModels.resetToleranceSeconds, 60)
        XCTAssertEqual(AnimalDisplayModels.frameCount, 24)
        XCTAssertEqual(AnimalDisplayModels.frameRate, 12)
        XCTAssertFalse(AnimalDisplayModels.defaultShowBurnAnimals)

        for c in fixture.phaseCases {
            let actual = AnimalDisplayModels.Phase.at(elapsedSeconds: c.phase)
            XCTAssertEqual(actual.content.rawValue, c.expectContent, "content — case: \(c.name)")
            if let actualProgress = actual.transitionProgress,
               let expectedProgress = c.expectTransitionProgress {
                XCTAssertEqual(actualProgress, expectedProgress, accuracy: 0.001,
                               "transition — case: \(c.name)")
            } else {
                XCTAssertEqual(actual.transitionProgress, c.expectTransitionProgress,
                               "transition — case: \(c.name)")
            }
            XCTAssertEqual(actual.percentageOpacity, c.expectPercentageOpacity, accuracy: 0.001,
                           "percentage opacity — case: \(c.name)")
            XCTAssertEqual(actual.animalOpacity, c.expectAnimalOpacity, accuracy: 0.001,
                           "animal opacity — case: \(c.name)")
            XCTAssertEqual(actual.frameIndex, c.expectFrameIndex, "frame — case: \(c.name)")
        }

        for c in fixture.assetCases {
            XCTAssertEqual(AnimalDisplayModels.assetID(for: c.emoji), c.expectAssetID,
                           "asset — case: \(c.name)")
        }
        XCTAssertNil(AnimalDisplayModels.assetID(for: "🐈"))

        for c in fixture.eligibilityCases {
            XCTAssertEqual(AnimalDisplayModels.eligibleAsset(showBurnAnimals: c.showBurnAnimals,
                isFiveHour: c.isFiveHour, emoji: c.animal, isActive: c.isActive), c.expectAssetID,
                "eligibility — case: \(c.name)")
        }

        for c in fixture.activityCases {
            var tracker = AnimalActivityTracker()
            for operation in c.operations where operation.kind == "success" {
                guard let utilization = operation.utilization else {
                    XCTFail("success operation missing utilization — case: \(c.name)")
                    continue
                }
                tracker.recordSuccess(accountID: operation.accountID, utilization: utilization,
                                      resetsAt: operation.resetsAt, at: operation.at)
            }
            for check in c.checks {
                XCTAssertEqual(tracker.isActive(accountID: check.accountID, at: check.at),
                               check.expectActive, "activity — case: \(c.name), at: \(check.at)")
            }
        }
    }

    func testCycleWrapsAtTenSeconds() {
        let start = AnimalDisplayModels.Phase.at(elapsedSeconds: 0)
        let wrap = AnimalDisplayModels.Phase.at(elapsedSeconds: 10)
        XCTAssertEqual(wrap.phase, start.phase)
        XCTAssertEqual(wrap.content, start.content)
    }
}
