import Foundation

/// One place for `contract/account-schema.md`'s "An unreadable store is not an
/// empty store", because the app's `AccountStore` and the helper's
/// `HelperAccountStore` persist to the *same* preferences suite and therefore
/// have to agree on where unreadable bytes go.
///
/// `UserDefaults` has only two failure shapes here, no value or a value that
/// will not decode, with no I/O tier between them. That is why the Linux port
/// has a branch this does not: see `contract/helper-cli.md`'s "add-key".
enum AccountStoreQuarantine {

    /// Decodes the store, and on a decode failure moves the bytes to a key of
    /// their own and names it in the result. Absent data is genuinely no
    /// accounts, with nothing to keep.
    ///
    /// Callers that only read must not use this: moving the bytes aside is a
    /// write, and they have nothing to protect by doing it.
    static func decodeForWrite(
        from defaults: UserDefaults,
        key: String
    ) -> (accounts: [Account], quarantined: String?) {
        guard let data = defaults.data(forKey: key) else { return ([], nil) }
        if let accounts = try? JSONDecoder().decode([Account].self, from: data) {
            return (accounts, nil)
        }
        // The timestamp is what keeps a second failure from overwriting the
        // copy the first one kept. Clearing the original key afterwards is
        // what leaves the store *absent* rather than corrupt, which is what
        // makes the caller's next write safe.
        let kept = "\(key).unreadable.\(Int(Date().timeIntervalSince1970))"
        defaults.set(data, forKey: kept)
        defaults.removeObject(forKey: key)
        return ([], kept)
    }
}
