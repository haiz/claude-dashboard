//! Test-only constructors shared by the helper's unit tests.
//!
//! `Account` deliberately has no `Default`: `AccountPlan` and `AccountStatus`
//! have no meaningful default variant, and inventing one in a model shared
//! with the macOS implementation would let a test-shaped convenience leak into
//! production decoding. Tests that need an account build one here instead.

use claude_dashboard_core::model::{Account, AccountPlan, AccountSource, AccountStatus, Browser};

/// An active, configured account. Callers mutate the fields they care about.
pub fn account(id: &str) -> Account {
    Account {
        id: id.to_string(),
        name: format!("{id}@example.com"),
        email: Some(format!("{id}@example.com")),
        chrome_profile_path: String::new(),
        chrome_profile_name: None,
        org_id: Some(format!("org-{id}")),
        account_uuid: None,
        session_key: None,
        browser: Browser::default(),
        plan: AccountPlan::Max200,
        last_synced: None,
        status: AccountStatus::Active,
        is_pinned: false,
        source: AccountSource::default(),
    }
}
