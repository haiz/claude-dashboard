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
}
