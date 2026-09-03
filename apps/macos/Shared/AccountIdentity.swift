import Foundation

/// One organisation the Claude account belongs to, as returned inside
/// `GET /api/account`'s `memberships[].organization`.
struct OrgMembership: Equatable {
    let uuid: String
    let name: String
    let capabilities: [String]

    var isChatOrg: Bool {
        capabilities.contains { $0.lowercased() == "chat" }
    }
}

/// The identity fields of an already-stored account, as dedupe sees them.
struct StoredIdentity: Equatable {
    let accountUuid: String?
    let email: String?

    init(accountUuid: String?, email: String?) {
        self.accountUuid = accountUuid
        self.email = email
    }

    init(_ account: Account) {
        self.init(accountUuid: account.accountUuid, email: account.email)
    }
}

/// The rules that decide *which account this is* and *which org to query usage
/// from*. Both are contract rules — `contract/cases/org-selection.json` and
/// `contract/cases/dedupe.json` drive them, and `apps/linux/core/src/identity.rs`
/// must satisfy the same two files.
///
/// These are pure functions on purpose: three call sites (the setup UI, the
/// macOS helper's `sync`, the Linux helper's `sync`) previously each carried
/// their own version and had drifted apart.
enum AccountIdentity {

    /// The org whose `/usage` the account should be polled against.
    ///
    /// 1. the cookie's `lastActiveOrg`, if that uuid is a chat org in `memberships`
    /// 2. otherwise the first chat org, in the order the API returned them
    /// 3. otherwise nil — the account is not configurable and must be reported,
    ///    never persisted as working
    ///
    /// The org *name* is never inspected. A personal org is eligible like any
    /// other; for a personal Pro or Max account it is the correct answer.
    static func resolveOrgId(lastActiveOrg: String?, memberships: [OrgMembership]) -> String? {
        if let lastActiveOrg,
           let cookieOrg = memberships.first(where: { $0.uuid == lastActiveOrg }),
           cookieOrg.isChatOrg {
            return cookieOrg.uuid
        }
        return memberships.first(where: \.isChatOrg)?.uuid
    }

    /// Which stored record `candidateUuid` names, if any — the position of the
    /// first match, in store order.
    ///
    /// 1. a stored `accountUuid` equal to the candidate's — duplicate
    /// 2. a stored record with **no** `accountUuid` whose email matches
    ///    case-insensitively — duplicate. Legacy records only; once a record
    ///    has been backfilled its uuid is authoritative.
    /// 3. otherwise not a duplicate
    ///
    /// `orgId` is never consulted. Colleagues share an org and are different
    /// accounts.
    ///
    /// `isDuplicate` is this predicate. The index exists because `sync`'s plan
    /// heal needs the record it matched, and re-deriving that match at the call
    /// site is exactly the drift `contract/cases/dedupe.json` exists to prevent.
    static func duplicateIndex(
        candidateUuid: String,
        candidateEmail: String?,
        against stored: [StoredIdentity]
    ) -> Int? {
        stored.firstIndex { entry in
            if let storedUuid = entry.accountUuid {
                return storedUuid == candidateUuid
            }
            guard let storedEmail = entry.email, let candidateEmail else { return false }
            return storedEmail.caseInsensitiveCompare(candidateEmail) == .orderedSame
        }
    }

    /// Whether `candidateUuid` names an account already in the store. See
    /// `duplicateIndex` for the rule.
    static func isDuplicate(
        candidateUuid: String,
        candidateEmail: String?,
        against stored: [StoredIdentity]
    ) -> Bool {
        duplicateIndex(
            candidateUuid: candidateUuid, candidateEmail: candidateEmail, against: stored) != nil
    }
}
