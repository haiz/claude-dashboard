import CoreGraphics
import Foundation
import ImageIO

/// Loads the burn-animal sprite sheets (`Resources/BurnAnimals`) once and hands out
/// pre-cropped 128 x 128 frames, so a 12 fps gauge never decodes image data per tick.
///
/// Contract: the manifest is validated once against `AnimalDisplayModels` (schema 1,
/// 24 frames, 12 fps, numeric `columns`/`rows`, sheet size = grid x cell size). Each sheet
/// is decoded on first use, checked against `sheetWidth`/`sheetHeight`, and split row-major
/// from the top-left (frame f = column f % columns, row f / columns). Anything missing or
/// malformed yields nil, and the caller keeps the percentage visible; failures are cached
/// too, so a broken asset is not retried every frame.
///
///     let catalog = BurnAnimalAssetCatalog.shared
///     catalog.frame(7, of: "rabbit")      // CGImage, or nil -> draw the percentage only
///
/// Tests pass their own `directory` (a temp dir) instead of the app bundle.
@MainActor
final class BurnAnimalAssetCatalog {
    static let shared = BurnAnimalAssetCatalog(
        directory: Bundle.main.url(forResource: "BurnAnimals", withExtension: nil))

    struct Manifest: Decodable, Equatable {
        struct Animal: Decodable, Equatable {
            let id: String
            let sheet: String
        }
        let schemaVersion: Int
        let frameCount: Int
        let framesPerSecond: Int
        let columns: Int
        let rows: Int
        let cellWidth: Int
        let cellHeight: Int
        let sheetWidth: Int
        let sheetHeight: Int
        let animals: [Animal]
    }

    private let directory: URL?
    private var loadedManifest = false
    private var manifestCache: Manifest?
    private var frameCache: [String: [CGImage]?] = [:]

    init(directory: URL?) {
        self.directory = directory
    }

    /// The validated manifest, or nil when it is missing or does not match the contract.
    var manifest: Manifest? {
        if !loadedManifest {
            loadedManifest = true
            manifestCache = Self.loadManifest(in: directory)
        }
        return manifestCache
    }

    /// Frame `index` (0..<24) of `assetID`, or nil when the asset is unknown or unusable.
    func frame(_ index: Int, of assetID: String) -> CGImage? {
        guard let frames = frames(for: assetID), frames.indices.contains(index) else { return nil }
        return frames[index]
    }

    /// All frames of `assetID` in play order, decoded on first request and cached.
    func frames(for assetID: String) -> [CGImage]? {
        if let cached = frameCache[assetID] { return cached }
        let loaded = loadFrames(for: assetID)
        frameCache[assetID] = .some(loaded)
        return loaded
    }

    private func loadFrames(for assetID: String) -> [CGImage]? {
        guard let manifest, let directory,
              let entry = manifest.animals.first(where: { $0.id == assetID }) else { return nil }
        let url = directory.appendingPathComponent(entry.sheet)
        let options = [kCGImageSourceShouldCacheImmediately: true] as CFDictionary
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let sheet = CGImageSourceCreateImageAtIndex(source, 0, options),
              sheet.width == manifest.sheetWidth, sheet.height == manifest.sheetHeight else { return nil }

        var frames: [CGImage] = []
        frames.reserveCapacity(manifest.frameCount)
        for f in 0..<manifest.frameCount {
            let cell = CGRect(x: (f % manifest.columns) * manifest.cellWidth,
                              y: (f / manifest.columns) * manifest.cellHeight,
                              width: manifest.cellWidth, height: manifest.cellHeight)
            guard let frame = Self.copyCell(cell, of: sheet) else { return nil }
            frames.append(frame)
        }
        return frames
    }

    /// Draws one cell into its own bitmap so the frame owns decoded pixels (a plain
    /// `cropping(to:)` may keep re-reading the whole sheet when drawn).
    private static func copyCell(_ cell: CGRect, of sheet: CGImage) -> CGImage? {
        guard let cropped = sheet.cropping(to: cell),
              let context = CGContext(
                data: nil, width: Int(cell.width), height: Int(cell.height),
                bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.interpolationQuality = .none
        context.draw(cropped, in: CGRect(origin: .zero, size: cell.size))
        return context.makeImage()
    }

    static func loadManifest(in directory: URL?) -> Manifest? {
        guard let directory,
              let data = try? Data(contentsOf: directory.appendingPathComponent("manifest.json")),
              let manifest = try? JSONDecoder().decode(Manifest.self, from: data),
              isValid(manifest) else { return nil }
        return manifest
    }

    static func isValid(_ m: Manifest) -> Bool {
        m.schemaVersion == 1
            && m.frameCount == AnimalDisplayModels.frameCount
            && Double(m.framesPerSecond) == AnimalDisplayModels.frameRate
            && m.columns > 0 && m.rows > 0 && m.cellWidth > 0 && m.cellHeight > 0
            && m.columns * m.rows >= m.frameCount
            && m.sheetWidth == m.columns * m.cellWidth
            && m.sheetHeight == m.rows * m.cellHeight
            && !m.animals.isEmpty
            // A sheet is a bare file name inside the directory, never a path.
            && m.animals.allSatisfy { !$0.sheet.isEmpty && !$0.sheet.contains("/") && $0.sheet != ".." }
    }
}
