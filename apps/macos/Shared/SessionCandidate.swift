import Foundation

/// One session key, identified against `/api/account` and matched against the
/// store — a verdict, not a write.
///
/// The callers that need this have three different behaviours on a duplicate:
/// the setup scan skips it, `sync` heals only its plan tier, and the manual-key
/// paths repair it. A shared function that performed the write would need a mode
/// flag to serve them, so it returns what it found and the caller decides.
struct SessionCandidate {
    /// The account this session belongs to, from `GET /api/account`.
    let identity: AccountInfo
    /// `GET /api/organizations`, the plan-tier source, or nil when that call
    /// failed. A failure here is not fatal: session validity is established by
    /// `/api/account`, and the plan falls back.
    let orgs: [OrgInfo]?
    /// The position of the stored record this session names, per
    /// `contract/cases/dedupe.json`, or nil when it names none.
    let duplicateIndex: Int?

    /// nil when the session is unusable — expired, revoked, or unreachable.
    static func validate(
        sessionKey: String,
        against stored: [StoredIdentity],
        apiService: UsageAPIService
    ) async -> SessionCandidate? {
        guard let identity = try? await apiService.fetchAccount(sessionKey: sessionKey) else {
            return nil
        }
        let orgs = try? await apiService.fetchOrganizations(sessionKey: sessionKey)
        let index = AccountIdentity.duplicateIndex(
            candidateUuid: identity.uuid,
            candidateEmail: identity.email,
            against: stored)
        return SessionCandidate(identity: identity, orgs: orgs, duplicateIndex: index)
    }
}
