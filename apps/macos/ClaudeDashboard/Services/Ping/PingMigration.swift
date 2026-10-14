import Foundation

/// Removes what the Run Command feature left behind: its log database and its
/// UserDefaults keys. Runs at every launch; each step is idempotent, so no
/// "migrated" flag is stored. Nothing is carried over: Auto Ping starts off.
///
///     PingMigration.run(defaults: AppDefaults.shared,
///                       logsDirectory: PingLogStore.defaultDirectory())
enum PingMigration {
    /// `runCommandTerminal_` does not start with `runCommand_`, so both are listed.
    static let legacyKeyPrefixes = ["runCommand_", "runCommandTerminal_"]
    static let legacyKeys: Set<String> = ["menuBarShowRunCommand"]
    static let legacyLogFiles = ["command_logs.db", "command_logs.db-wal", "command_logs.db-shm"]

    static func run(defaults: UserDefaults, fileManager: FileManager = .default, logsDirectory: URL) {
        for file in legacyLogFiles {
            try? fileManager.removeItem(at: logsDirectory.appendingPathComponent(file))
        }
        for key in defaults.dictionaryRepresentation().keys
        where legacyKeys.contains(key) || legacyKeyPrefixes.contains(where: { key.hasPrefix($0) }) {
            defaults.removeObject(forKey: key)
        }
    }
}
