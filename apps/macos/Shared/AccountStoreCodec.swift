import Foundation

/// Reads and writes the stored accounts array one record at a time, for
/// `contract/account-schema.md`'s "A record this build cannot read". A record a
/// newer build wrote (a browser, plan, status or source this build has no case
/// for) costs only that record: it is left out of `accounts`, and every write
/// puts it back untouched.
///
///     guard let decoded = AccountStoreCodec.decode(data) else {
///         // Not a JSON array at all: the whole store is unreadable.
///     }
///     use(decoded.accounts)
///     let bytes = try AccountStoreCodec.encode(accounts, over: defaults.data(forKey: key))
///
/// Writers keep the records by reading them back from the bytes they are about
/// to replace, so neither store has to carry them between load and save.
enum AccountStoreCodec {

    struct Decoded {
        /// The records this build can read, in stored order.
        let accounts: [Account]
        /// The rest, as `JSONSerialization` objects, in stored order.
        let unreadable: [Any]
    }

    /// Nil only when `data` is not a JSON array; a bad record never fails the
    /// whole decode.
    static func decode(_ data: Data) -> Decoded? {
        guard let records = (try? JSONSerialization.jsonObject(with: data)) as? [Any] else {
            return nil
        }
        var accounts: [Account] = []
        var unreadable: [Any] = []
        for record in records {
            // `data(withJSONObject:)` raises (uncatchable) on a bare scalar.
            if JSONSerialization.isValidJSONObject(record),
               let recordData = try? JSONSerialization.data(withJSONObject: record),
               let account = try? JSONDecoder().decode(Account.self, from: recordData) {
                accounts.append(account)
            } else {
                unreadable.append(record)
            }
        }
        return Decoded(accounts: accounts, unreadable: unreadable)
    }

    /// `accounts`, followed by the records of `existing` this build cannot read.
    /// With nothing to keep, this is exactly `JSONEncoder().encode(accounts)`.
    static func encode(_ accounts: [Account], over existing: Data?) throws -> Data {
        let encoded = try JSONEncoder().encode(accounts)
        guard let kept = existing.flatMap(decode)?.unreadable, !kept.isEmpty else {
            return encoded
        }
        let readable = try JSONSerialization.jsonObject(with: encoded) as? [Any] ?? []
        return try JSONSerialization.data(withJSONObject: readable + kept)
    }
}
