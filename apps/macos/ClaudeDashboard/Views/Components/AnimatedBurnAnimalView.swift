import SwiftUI

/// What a 5h gauge needs to animate its burn animal: the one shared clock (never a
/// per-card one) and this account's eligibility. Built only by `DashboardViewModel`
/// (`burnAnimalSource(for:)`) and passed to the 5h `UsageBar` alone; 7d / Fable get nil.
///
///     UsageBar(label: "5h", ..., burnAnimal: viewModel.burnAnimalSource(for: state))
struct BurnAnimalSource {
    let clock: AnimalDisplayClock
    /// The asset id to draw now, or nil. Re-asked on every clock tick, so the 600 s
    /// activity window lapses without waiting for a refresh.
    let assetID: () -> String?
}

/// The fixed center of a usage ring: the percentage and, when a source is given and
/// eligible, the animated animal, both in one `side` x `side` box so nothing moves.
///
///     GaugeCenter(source: burnAnimal, side: 40) { Text("62%") }
struct GaugeCenter<Percentage: View>: View {
    let source: BurnAnimalSource?
    let side: CGFloat
    @ViewBuilder let percentage: () -> Percentage

    var body: some View {
        if let source {
            ObservingGaugeCenter(clock: source.clock, assetID: source.assetID, side: side, percentage: percentage)
        } else {
            GaugeCenterLayers(phase: nil, frame: nil, side: side, percentage: percentage)
        }
    }
}

/// Subscribes to the shared clock; only gauges that were handed a source re-render at 12 fps.
private struct ObservingGaugeCenter<Percentage: View>: View {
    @ObservedObject var clock: AnimalDisplayClock
    let assetID: () -> String?
    let side: CGFloat
    let percentage: () -> Percentage

    var body: some View {
        let phase = clock.phase
        let frame = clock.isRunning
            ? GaugeCenterLayers<Percentage>.frame(for: phase, assetID: assetID(), catalog: .shared)
            : nil
        GaugeCenterLayers(phase: phase, frame: frame, side: side, percentage: percentage)
    }
}

/// Pure rendering of one phase (the clock-free half, which preview tests render directly).
struct GaugeCenterLayers<Percentage: View>: View {
    let phase: AnimalDisplayModels.Phase?
    /// The animal frame to draw; nil keeps the percentage fully visible.
    let frame: CGImage?
    let side: CGFloat
    let percentage: () -> Percentage

    /// Percentage and animal opacity. Without a frame the percentage stays at 1.
    static func opacities(phase: AnimalDisplayModels.Phase?, hasFrame: Bool) -> (percentage: Double, animal: Double) {
        guard hasFrame, let phase, phase.frameIndex != nil else { return (1, 0) }
        return (phase.percentageOpacity, phase.animalOpacity)
    }

    /// The cached frame for this phase, or nil (no asset, percentage-only phase, bad asset).
    @MainActor
    static func frame(for phase: AnimalDisplayModels.Phase, assetID: String?,
                      catalog: BurnAnimalAssetCatalog) -> CGImage? {
        guard let assetID, let index = phase.frameIndex else { return nil }
        return catalog.frame(index, of: assetID)
    }

    /// Each layer grows from 85% as it fades in, driven by the same phase (no implicit animation).
    private static func scale(_ opacity: Double) -> CGFloat { 0.85 + 0.15 * opacity }

    var body: some View {
        let o = Self.opacities(phase: phase, hasFrame: frame != nil)
        ZStack {
            percentage()
                .opacity(o.percentage)
                .scaleEffect(Self.scale(o.percentage))
            if let frame, o.animal > 0 {
                Image(decorative: frame, scale: 1)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .opacity(o.animal)
                    .scaleEffect(Self.scale(o.animal))
                    .accessibilityHidden(true)
            }
        }
        .frame(width: side, height: side)
    }
}
