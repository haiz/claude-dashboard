import XCTest
import ImageIO
import UniformTypeIdentifiers
@testable import ClaudeDashboard

@MainActor
final class BurnAnimalAssetCatalogTests: XCTestCase {
    private static let ids = ["snail", "turtle", "rabbit", "horse", "cheetah"]
    private var dir: URL!

    private var bundleDir: URL {
        get throws { try XCTUnwrap(Bundle.main.url(forResource: "BurnAnimals", withExtension: nil),
                                   "the app bundle must contain Resources/BurnAnimals") }
    }

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("BurnAnimalAssetCatalogTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: dir)
        super.tearDown()
    }

    /// The bundled manifest as a mutable JSON object.
    private func bundledManifest() throws -> [String: Any] {
        let data = try Data(contentsOf: bundleDir.appendingPathComponent("manifest.json"))
        return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    private func writeManifest(_ object: [String: Any]) throws {
        try JSONSerialization.data(withJSONObject: object)
            .write(to: dir.appendingPathComponent("manifest.json"))
    }

    private func copySheet(_ id: String) throws {
        try FileManager.default.copyItem(at: bundleDir.appendingPathComponent("\(id).png"),
                                         to: dir.appendingPathComponent("\(id).png"))
    }

    // MARK: - Bundle

    func testBundledManifestAndEverySheetLoad24Frames() throws {
        let catalog = BurnAnimalAssetCatalog(directory: try bundleDir)
        let manifest = try XCTUnwrap(catalog.manifest)
        XCTAssertEqual(manifest.animals.map(\.id), Self.ids)
        for id in Self.ids {
            let frames = try XCTUnwrap(catalog.frames(for: id), id)
            XCTAssertEqual(frames.count, AnimalDisplayModels.frameCount, id)
            XCTAssertTrue(frames.allSatisfy { $0.width == 128 && $0.height == 128 }, id)
        }
        // Every emoji the tracker can emit maps to a bundled animal.
        for emoji in BurnRateResult.animals.values {
            let id = try XCTUnwrap(AnimalDisplayModels.assetID(for: emoji), emoji)
            XCTAssertNotNil(catalog.frame(0, of: id), id)
        }
    }

    func testFramesAreRowMajorCells() throws {
        try writeManifest(try bundledManifest())
        try copySheet("rabbit")
        let catalog = BurnAnimalAssetCatalog(directory: dir)
        let frames = try XCTUnwrap(catalog.frames(for: "rabbit"))
        // Frame 7 is column 1, row 1 of the sheet.
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(dir.appendingPathComponent("rabbit.png") as CFURL, nil))
        let sheet = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
        let cell = try XCTUnwrap(sheet.cropping(to: CGRect(x: 128, y: 128, width: 128, height: 128)))
        XCTAssertEqual(alphaSum(frames[7]), alphaSum(cell), accuracy: 1)
        XCTAssertNil(catalog.frame(24, of: "rabbit"))
    }

    // MARK: - Failures fall back to nil

    func testMalformedJSONIsNil() throws {
        try Data("{ not json".utf8).write(to: dir.appendingPathComponent("manifest.json"))
        try copySheet("snail")
        let catalog = BurnAnimalAssetCatalog(directory: dir)
        XCTAssertNil(catalog.manifest)
        XCTAssertNil(catalog.frame(0, of: "snail"))
    }

    func testManifestWithWrongDimensionsIsNil() throws {
        var m = try bundledManifest()
        m["sheetWidth"] = 700
        try writeManifest(m)
        try copySheet("snail")
        XCTAssertNil(BurnAnimalAssetCatalog(directory: dir).frame(0, of: "snail"))

        m = try bundledManifest()
        m["frameCount"] = 20
        try writeManifest(m)
        XCTAssertNil(BurnAnimalAssetCatalog(directory: dir).frame(0, of: "snail"))
    }

    func testSheetWithWrongPixelSizeIsNil() throws {
        try writeManifest(try bundledManifest())
        try writePNG(width: 64, height: 64, to: dir.appendingPathComponent("snail.png"))
        let catalog = BurnAnimalAssetCatalog(directory: dir)
        XCTAssertNotNil(catalog.manifest)
        XCTAssertNil(catalog.frames(for: "snail"))
    }

    func testUnknownIdIsNil() throws {
        try writeManifest(try bundledManifest())
        try copySheet("snail")
        let catalog = BurnAnimalAssetCatalog(directory: dir)
        XCTAssertNil(catalog.frame(0, of: "dragon"))
        XCTAssertNotNil(catalog.frame(0, of: "snail"))
    }

    func testMissingSheetIsNil() throws {
        try writeManifest(try bundledManifest())
        let catalog = BurnAnimalAssetCatalog(directory: dir)
        XCTAssertNil(catalog.frame(0, of: "turtle"))
        XCTAssertNil(BurnAnimalAssetCatalog(directory: nil).frame(0, of: "turtle"))
    }

    func testSheetPathOutsideDirectoryIsRejected() throws {
        var m = try bundledManifest()
        m["animals"] = [["id": "snail", "sheet": "../snail.png"]]
        try writeManifest(m)
        XCTAssertNil(BurnAnimalAssetCatalog(directory: dir).manifest)
    }

    // MARK: - Helpers

    private func alphaSum(_ image: CGImage) -> Double {
        let w = image.width, h = image.height
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let ctx = CGContext(data: &pixels, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        return stride(from: 3, to: pixels.count, by: 4).reduce(0) { $0 + Double(pixels[$1]) } / 255
    }

    private func writePNG(width: Int, height: Int, to url: URL) throws {
        let ctx = try XCTUnwrap(CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let image = try XCTUnwrap(ctx.makeImage())
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
    }
}
