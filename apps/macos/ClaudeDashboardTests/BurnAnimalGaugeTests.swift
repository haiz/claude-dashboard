import XCTest
import SwiftUI
import AppKit
@testable import ClaudeDashboard

/// The 5h ring's center layers: contract opacities, percentage fallback, no layout shift.
/// With `TEST_RUNNER_BURN_ANIMAL_PREVIEW_DIR=<dir>` set on `xcodebuild test`, it also writes
/// compact/full light/dark PNGs of the gauge at four phases there for a visual check.
@MainActor
final class BurnAnimalGaugeTests: XCTestCase {
    private typealias Layers = GaugeCenterLayers<EmptyView>
    private static let phases: [Double] = [3.0, 5.75, 7.0, 9.75]

    // MARK: - Opacities

    func testOpacitiesFollowTheSharedPhase() {
        for t in [0, 3.0, 5.5, 5.75, 6.0, 7.0, 9.5, 9.75, 9.99] {
            let phase = AnimalDisplayModels.Phase.at(elapsedSeconds: t)
            let o = Layers.opacities(phase: phase, hasFrame: true)
            if phase.frameIndex == nil {
                XCTAssertEqual(o.percentage, 1); XCTAssertEqual(o.animal, 0)
            } else {
                XCTAssertEqual(o.percentage, phase.percentageOpacity, "t=\(t)")
                XCTAssertEqual(o.animal, phase.animalOpacity, "t=\(t)")
            }
        }
        let mid = Layers.opacities(phase: .at(elapsedSeconds: 5.75), hasFrame: true)
        XCTAssertEqual(mid.percentage, 0.5, accuracy: 1e-9)
        XCTAssertEqual(mid.animal, 0.5, accuracy: 1e-9)
    }

    func testWithoutAFrameThePercentageStaysFullyVisible() {
        for t in [3.0, 5.75, 7.0, 9.75] {
            let o = Layers.opacities(phase: .at(elapsedSeconds: t), hasFrame: false)
            XCTAssertEqual(o.percentage, 1, "t=\(t)")
            XCTAssertEqual(o.animal, 0, "t=\(t)")
        }
        XCTAssertEqual(Layers.opacities(phase: nil, hasFrame: true).percentage, 1)
    }

    func testFrameSelection_unknownAssetOrPercentagePhaseIsNil() {
        let catalog = BurnAnimalAssetCatalog.shared
        XCTAssertNil(Layers.frame(for: .at(elapsedSeconds: 3), assetID: "rabbit", catalog: catalog))
        XCTAssertNil(Layers.frame(for: .at(elapsedSeconds: 7), assetID: nil, catalog: catalog))
        XCTAssertNil(Layers.frame(for: .at(elapsedSeconds: 7), assetID: "dragon", catalog: catalog))
        XCTAssertNotNil(Layers.frame(for: .at(elapsedSeconds: 7), assetID: "rabbit", catalog: catalog))
        let broken = BurnAnimalAssetCatalog(directory: nil)
        XCTAssertNil(Layers.frame(for: .at(elapsedSeconds: 7), assetID: "rabbit", catalog: broken))
    }

    // MARK: - Layout

    /// A clock frozen at `elapsed` seconds into the cycle, running, nothing scheduled.
    private func frozenSource(at elapsed: Double, asset: String? = "rabbit") -> BurnAnimalSource {
        var t = 0.0
        let clock = AnimalDisplayClock(now: { t }, scheduler: { _, _ in {} })
        t = elapsed
        clock.shouldRun = { true }
        clock.gaugeAppeared()       // offscreen renders may never call onAppear
        return BurnAnimalSource(clock: clock, assetID: { asset })
    }

    private func gauge(_ source: BurnAnimalSource?, compact: Bool, utilization: Double = 62) -> some View {
        UsageBar(label: "5h", utilization: utilization, resetsAt: Date().addingTimeInterval(8_100),
                 totalSeconds: 18_000, burnAnimal: source, isCompact: compact)
    }

    func testRingSizeDoesNotChangeBetweenPhases() {
        for compact in [true, false] {
            let base = NSHostingController(rootView: gauge(nil, compact: compact)).view.fittingSize
            for t in Self.phases {
                let size = NSHostingController(rootView: gauge(frozenSource(at: t), compact: compact)).view.fittingSize
                XCTAssertEqual(size, base, "compact=\(compact) t=\(t)")
            }
        }
    }

    // MARK: - Preview PNGs (opt-in)

    func testWritePreviewPNGsWhenRequested() throws {
        guard let path = ProcessInfo.processInfo.environment["BURN_ANIMAL_PREVIEW_DIR"] else {
            throw XCTSkip("set TEST_RUNNER_BURN_ANIMAL_PREVIEW_DIR to write gauge previews")
        }
        let out = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        for scheme in [ColorScheme.light, .dark] {
            for compact in [true, false] {
                for (asset, utilization) in [("rabbit", 62.0), ("cheetah", 100.0)] {
                    let row = HStack(spacing: 12) {
                        gauge(nil, compact: compact, utilization: utilization)
                        ForEach(Self.phases, id: \.self) { t in
                            self.gauge(self.frozenSource(at: t, asset: asset), compact: compact, utilization: utilization)
                        }
                    }
                    .padding(12)
                    .background(scheme == .dark ? Color(white: 0.16) : Color(white: 0.97))
                    .environment(\.colorScheme, scheme)
                    let renderer = ImageRenderer(content: row)
                    renderer.scale = 3
                    let image = try XCTUnwrap(renderer.cgImage)
                    let name = "\(compact ? "compact52" : "full68")-\(scheme == .dark ? "dark" : "light")-\(asset).png"
                    let data = try XCTUnwrap(NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]))
                    try data.write(to: out.appendingPathComponent(name))
                }
            }
        }
    }
}
