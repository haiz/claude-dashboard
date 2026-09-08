import Foundation

/// The `UserDefaults` keys holding one account's saved run command.
///
/// Keyed by `Account.id`, which is minted per record: an account deleted and
/// scanned back in is a new id, so a command left under the old one is
/// unreachable by construction. That makes deleting the keys part of deleting
/// the account, and this is where all four call sites agree on their names.
enum RunCommandSettings {
    private static let commandPrefix = "runCommand_"
    private static let terminalPrefix = "runCommandTerminal_"

    static func commandKey(for accountId: UUID) -> String {
        commandPrefix + accountId.uuidString
    }

    static func terminalKey(for accountId: UUID) -> String {
        terminalPrefix + accountId.uuidString
    }

    /// Drops one account's saved command. Part of deleting the account.
    static func remove(for accountId: UUID, in defaults: UserDefaults) {
        defaults.removeObject(forKey: commandKey(for: accountId))
        defaults.removeObject(forKey: terminalKey(for: accountId))
    }

    /// Drops the commands of accounts that no longer exist.
    ///
    /// `remove` covers deletes from now on; this covers the ones already in
    /// the file, from a version that did not clean up. Loading the store is
    /// the only moment that knows every live id, which is why it lives there
    /// and not in a view.
    ///
    /// A key whose suffix is not a UUID is left alone: it was not written by
    /// `commandKey`, so it is not ours to delete.
    static func prune(keeping liveIds: Set<UUID>, in defaults: UserDefaults) {
        for key in defaults.dictionaryRepresentation().keys {
            let suffix: String
            if key.hasPrefix(commandPrefix) {
                suffix = String(key.dropFirst(commandPrefix.count))
            } else if key.hasPrefix(terminalPrefix) {
                suffix = String(key.dropFirst(terminalPrefix.count))
            } else {
                continue
            }
            guard let id = UUID(uuidString: suffix), !liveIds.contains(id) else { continue }
            defaults.removeObject(forKey: key)
        }
    }
}
