import Foundation

/// Loads files from the repo's `contract/` directory.
///
/// Resolves through `#filePath` rather than `Bundle(for:)` on purpose: the whole
/// point of the contract is that the Swift and Rust suites read the *same file on
/// disk*. Copying the cases into a test bundle would let the two drift apart
/// again, which is the failure this directory exists to prevent.
enum ContractFixtures {

    /// Repo root, derived from this file's location:
    /// <root>/apps/macos/ClaudeDashboardTests/ContractFixtures.swift
    private static var repoRoot: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()   // ClaudeDashboardTests
            .deletingLastPathComponent()   // macos
            .deletingLastPathComponent()   // apps
            .deletingLastPathComponent()   // <root>
    }

    static func url(_ relativePath: String) -> URL {
        repoRoot.appendingPathComponent("contract").appendingPathComponent(relativePath)
    }

    static func data(_ relativePath: String) throws -> Data {
        try Data(contentsOf: url(relativePath))
    }
}
