import Foundation

/// The fields of the stored record a pasted key matched, as this rule sees them.
struct StoredManualTarget: Equatable {
    let orgId: String?
    let accountUuid: String?
    let email: String?

    init(orgId: String?, accountUuid: String?, email: String?) {
        self.orgId = orgId
        self.accountUuid = accountUuid
        self.email = email
    }

    init(_ account: Account) {
        self.init(orgId: account.orgId, accountUuid: account.accountUuid, email: account.email)
    }
}

/// Fields the repair branch writes. `nil` means **leave the stored value alone**.
struct ManualKeyWrites: Equatable {
    let orgId: String?
    let accountUuid: String?
    let email: String?
}

enum ManualKeyDecision: Equatable {
    case add(orgId: String)
    case rejectNoChatOrg
    case repair(writes: ManualKeyWrites)
}

/// What a pasted session key does to the store.
///
/// Contract rule — `contract/cases/manual-key.json` drives this, and
/// `apps/linux/core/src/manual_key.rs` must satisfy the same file. Dedupe
/// (`contract/cases/dedupe.json`) decides *whether* a stored record matches;
/// this decides what happens next.
enum ManualKey {

    /// 1. No stored match: add the account at the org `resolveOrgId` picks with
    ///    no `lastActiveOrg` preference (rule 2 of org selection). No chat org
    ///    at all rejects it — an unresolvable org is never persisted as working.
    /// 2. A stored match: repair. `orgId` is **not** rewritten, because a pasted
    ///    key carries no org preference and re-resolving would demote an `orgId`
    ///    the cookie had resolved correctly. The exception is a stored `nil`:
    ///    nothing to demote, same backfill semantics as `accountUuid`.
    /// 3. A repair is never rejected for having no chat org. The key is still
    ///    saved, mirroring `resyncCore`, and the caller reports the org.
    static func decision(
        stored: StoredManualTarget?,
        fetchedUuid: String,
        fetchedEmail: String?,
        memberships: [OrgMembership]
    ) -> ManualKeyDecision {
        let resolved = AccountIdentity.resolveOrgId(lastActiveOrg: nil, memberships: memberships)

        guard let stored else {
            guard let resolved else { return .rejectNoChatOrg }
            return .add(orgId: resolved)
        }

        return .repair(writes: ManualKeyWrites(
            orgId: stored.orgId == nil ? resolved : nil,
            accountUuid: stored.accountUuid == nil ? fetchedUuid : nil,
            email: stored.email == nil ? fetchedEmail : nil
        ))
    }
}
