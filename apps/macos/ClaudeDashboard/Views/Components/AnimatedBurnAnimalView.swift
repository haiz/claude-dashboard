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
/// Its mount count gates the clock: no gauge on screen, no timer.
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
            .onAppear { clock.gaugeAppeared() }
            .onDisappear { clock.gaugeDisappeared() }
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

    /// Each layer grows from 85% as it fades in, driven by the same phase.
    private static func scale(_ opacity: Double) -> CGFloat { 0.85 + 0.15 * opacity }

    /// The clock steps 12 times a second; each layer's opacity and scale glide linearly across
    /// one step so the 500 ms cross-fades read smooth. Scoped to those two modifiers: the
    /// `transaction` below them keeps the layer's own content (text, frame image, layout)
    /// from animating.
    private static var step: Animation { .linear(duration: 1 / AnimalDisplayModels.frameRate) }

    var body: some View {
        let o = Self.opacities(phase: phase, hasFrame: frame != nil)
        ZStack {
            percentage()
                .transaction { $0.animation = nil }
                .opacity(o.percentage)
                .scaleEffect(Self.scale(o.percentage))
                .animation(Self.step, value: o.percentage)
            if let frame, o.animal > 0 {
                Image(decorative: frame, scale: 1)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .transaction { $0.animation = nil }
                    .opacity(o.animal)
                    .scaleEffect(Self.scale(o.animal))
                    .animation(Self.step, value: o.animal)
                    .accessibilityHidden(true)
            }
        }
        .frame(width: side, height: side)
    }
}
