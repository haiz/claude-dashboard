import Foundation
import AppKit

struct UpdateInfo: Equatable {
    let version: String
    let downloadURL: URL
    let body: String?
}

enum UpdateError: LocalizedError {
    case networkError(String)
    case assetNotFound
    case unzipFailed

    var errorDescription: String? {
        switch self {
        case .networkError(let msg): return "Network error: \(msg)"
        case .assetNotFound: return "Update package not found in release"
        case .unzipFailed: return "Failed to extract update"
        }
    }
}

final class UpdateService {
    private let session: URLSession
    let currentVersion: String
    private let repoSlug = "haiz/claude-dashboard"

    init(session: URLSession = .shared, currentVersion: String = AppVersion.string) {
        self.session = session
        self.currentVersion = currentVersion
    }

    func checkForUpdate() async throws -> UpdateInfo? {
        let url = URL(string: "https://api.github.com/repos/\(repoSlug)/releases/latest")!
        var request = URLRequest(url: url)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")

        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw UpdateError.networkError("Unexpected HTTP response")
        }

        let release = try JSONDecoder().decode(GitHubRelease.self, from: data)
        let remote = release.tagName.hasPrefix("v") ? String(release.tagName.dropFirst()) : release.tagName

        guard isNewer(remote: remote, than: currentVersion) else { return nil }

        guard let asset = release.assets.first(where: { $0.name == "ClaudeDashboard.app.zip" }),
              let downloadURL = URL(string: asset.browserDownloadURL) else {
            throw UpdateError.assetNotFound
        }

        return UpdateInfo(version: remote, downloadURL: downloadURL, body: release.body)
    }

    func download(_ info: UpdateInfo) async throws -> URL {
        let (tmpURL, _) = try await session.download(from: info.downloadURL)
        let stableURL = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("ClaudeDashboard-update-\(UUID().uuidString).zip")
        try FileManager.default.moveItem(at: tmpURL, to: stableURL)
        return stableURL
    }

    func installAndRelaunch(zipURL: URL) async throws {
        let bundleURL = Bundle.main.bundleURL
        let stagingDir = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("cd-update-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: stagingDir, withIntermediateDirectories: true)

        let zipPath = zipURL.path
        let stagingPath = stagingDir.path
        try await Task.detached(priority: .userInitiated) {
            let unzip = Process()
            unzip.launchPath = "/usr/bin/ditto"
            unzip.arguments = ["-x", "-k", zipPath, stagingPath]
            try unzip.run()
            unzip.waitUntilExit()
            guard unzip.terminationStatus == 0 else { throw UpdateError.unzipFailed }
        }.value

        let stagedApp = stagingDir.appendingPathComponent("ClaudeDashboard.app")
        guard FileManager.default.fileExists(atPath: stagedApp.path) else {
            throw UpdateError.unzipFailed
        }

        let scriptURL = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("cd-relauncher-\(UUID().uuidString).sh")
        let logFile = URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("Library/Logs/ClaudeDashboard-relauncher.log")
        let pid = ProcessInfo.processInfo.processIdentifier

        let script = """
        #!/bin/bash
        exec >> "\(logFile.path)" 2>&1
        echo "[$(date)] Relauncher started: pid=\(pid) staged=\(stagedApp.path) target=\(bundleURL.path)"
        for i in $(seq 1 60); do
            kill -0 \(pid) 2>/dev/null || break
            sleep 0.2
        done
        rm -rf "\(bundleURL.path)"
        mv "\(stagedApp.path)" "\(bundleURL.path)"
        xattr -cr "\(bundleURL.path)"
        open "\(bundleURL.path)"
        echo "[$(date)] Relauncher done."
        """

        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: NSNumber(value: Int16(0o755))],
            ofItemAtPath: scriptURL.path
        )

        let launcher = Process()
        launcher.launchPath = "/bin/bash"
        launcher.arguments = [scriptURL.path]
        try launcher.run()
    }

    func isNewer(remote: String, than current: String) -> Bool {
        let r = remote.split(separator: ".").compactMap { Int($0) }
        let c = current.split(separator: ".").compactMap { Int($0) }
        let count = max(r.count, c.count)
        for i in 0..<count {
            let rv = i < r.count ? r[i] : 0
            let cv = i < c.count ? c[i] : 0
            if rv > cv { return true }
            if rv < cv { return false }
        }
        return false
    }
}

// MARK: - GitHub API models

private struct GitHubRelease: Decodable {
    let tagName: String
    let body: String?
    let assets: [GitHubAsset]

    enum CodingKeys: String, CodingKey {
        case tagName = "tag_name"
        case body
        case assets
    }
}

private struct GitHubAsset: Decodable {
    let name: String
    let browserDownloadURL: String

    enum CodingKeys: String, CodingKey {
        case name
        case browserDownloadURL = "browser_download_url"
    }
}
