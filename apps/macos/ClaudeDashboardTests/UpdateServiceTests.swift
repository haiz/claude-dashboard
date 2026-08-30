import XCTest
@testable import ClaudeDashboard

final class UpdateServiceTests: XCTestCase {

    // MARK: - Version comparison

    func testIsNewer_remotePatchHigher() {
        let svc = UpdateService(currentVersion: "1.4.1")
        XCTAssertTrue(svc.isNewer(remote: "1.4.2", than: "1.4.1"))
    }

    func testIsNewer_remoteMinorHigher() {
        let svc = UpdateService(currentVersion: "1.4.1")
        XCTAssertTrue(svc.isNewer(remote: "1.5.0", than: "1.4.10"))
    }

    func testIsNewer_remoteMajorHigher() {
        let svc = UpdateService(currentVersion: "1.99.0")
        XCTAssertTrue(svc.isNewer(remote: "2.0.0", than: "1.99.0"))
    }

    func testIsNewer_equal_returnsFalse() {
        let svc = UpdateService(currentVersion: "1.4.1")
        XCTAssertFalse(svc.isNewer(remote: "1.4.1", than: "1.4.1"))
    }

    func testIsNewer_older_returnsFalse() {
        let svc = UpdateService(currentVersion: "1.4.1")
        XCTAssertFalse(svc.isNewer(remote: "1.4.0", than: "1.4.1"))
        XCTAssertFalse(svc.isNewer(remote: "0.9.9", than: "1.4.1"))
    }

    // MARK: - checkForUpdate: up-to-date

    func testCheckForUpdate_returnsNilWhenUpToDate() async throws {
        let json = releaseJSON(tag: "v1.4.1", assetName: "ClaudeDashboard.app.zip")
        let session = makeSession(json: json)
        let svc = UpdateService(session: session, currentVersion: "1.4.1")

        let result = try await svc.checkForUpdate()
        XCTAssertNil(result)
    }

    // MARK: - checkForUpdate: newer version found

    func testCheckForUpdate_returnsInfoWhenNewer() async throws {
        let json = releaseJSON(tag: "v1.5.0", assetName: "ClaudeDashboard.app.zip")
        let session = makeSession(json: json)
        let svc = UpdateService(session: session, currentVersion: "1.4.1")

        let info = try await svc.checkForUpdate()
        XCTAssertNotNil(info)
        XCTAssertEqual(info?.version, "1.5.0")
        XCTAssertTrue(info?.downloadURL.absoluteString.contains("ClaudeDashboard.app.zip") ?? false)
    }

    func testCheckForUpdate_stripsVPrefix() async throws {
        let json = releaseJSON(tag: "v2.0.0", assetName: "ClaudeDashboard.app.zip")
        let session = makeSession(json: json)
        let svc = UpdateService(session: session, currentVersion: "1.4.1")

        let info = try await svc.checkForUpdate()
        XCTAssertEqual(info?.version, "2.0.0")
    }

    func testCheckForUpdate_throwsWhenAssetMissing() async throws {
        let json = releaseJSON(tag: "v2.0.0", assetName: "SomethingElse.zip")
        let session = makeSession(json: json)
        let svc = UpdateService(session: session, currentVersion: "1.4.1")

        do {
            _ = try await svc.checkForUpdate()
            XCTFail("Expected error")
        } catch UpdateError.assetNotFound {
            // expected
        }
    }

    // MARK: - Helpers

    private func releaseJSON(tag: String, assetName: String) -> Data {
        """
        {
          "tag_name": "\(tag)",
          "body": "Release notes here.",
          "assets": [
            {
              "name": "\(assetName)",
              "browser_download_url": "https://github.com/haiz/claude-dashboard/releases/download/\(tag)/\(assetName)"
            }
          ]
        }
        """.data(using: .utf8)!
    }

    private func makeSession(json: Data, statusCode: Int = 200) -> URLSession {
        MockURLProtocol.requestHandler = { _ in
            let resp = HTTPURLResponse(
                url: URL(string: "https://api.github.com")!,
                statusCode: statusCode,
                httpVersion: nil,
                headerFields: nil
            )!
            return (resp, json)
        }
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        return URLSession(configuration: config)
    }
}
