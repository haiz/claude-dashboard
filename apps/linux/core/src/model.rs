use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountPlan {
    #[serde(rename = "Pro")]
    Pro,
    #[serde(rename = "Max 5x")]
    Max5x,
    #[serde(rename = "Max 20x")]
    Max20x,
    #[serde(rename = "Max")]
    Max200,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "error")]
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum Browser {
    #[default]
    #[serde(rename = "chrome")]
    Chrome,
    #[serde(rename = "arc")]
    Arc,
    #[serde(rename = "brave")]
    Brave,
    #[serde(rename = "edge")]
    Edge,
}

/// Where the record's session key comes from. Mirrors Swift's `AccountSource`;
/// `manual` means the user pasted it and there is no browser profile behind the
/// record. Consumers key on this field alone, never on an empty
/// `chrome_profile_path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccountSource {
    #[serde(rename = "browser")]
    #[default]
    Browser,
    #[serde(rename = "manual")]
    Manual,
}

const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0; // 2001-01-01 -> 1970-01-01

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub email: Option<String>,
    #[serde(rename = "chromeProfilePath")]
    pub chrome_profile_path: String,
    #[serde(rename = "chromeProfileName", skip_serializing_if = "Option::is_none", default)]
    pub chrome_profile_name: Option<String>,
    #[serde(rename = "orgId", skip_serializing_if = "Option::is_none", default)]
    pub org_id: Option<String>,
    #[serde(rename = "accountUuid", skip_serializing_if = "Option::is_none", default)]
    pub account_uuid: Option<String>,
    #[serde(rename = "sessionKey", skip_serializing_if = "Option::is_none", default)]
    pub session_key: Option<String>,
    #[serde(default)]
    pub browser: Browser, // missing -> Chrome (Default)
    pub plan: AccountPlan,
    #[serde(rename = "lastSynced", skip_serializing_if = "Option::is_none", default)]
    pub last_synced: Option<f64>, // reference-date seconds
    pub status: AccountStatus,
    #[serde(rename = "isPinned", default)]
    pub is_pinned: bool, // missing -> false
    #[serde(default)]
    pub source: AccountSource, // missing -> Browser (Default)
}

impl Account {
    pub fn from_json_object(s: &str) -> serde_json::Result<Account> {
        serde_json::from_str(s)
    }
    pub fn from_json(s: &str) -> serde_json::Result<Vec<Account>> {
        serde_json::from_str(s)
    }
    pub fn to_json_array(accounts: &[Account]) -> serde_json::Result<String> {
        serde_json::to_string(accounts)
    }
    pub fn last_synced_unix(&self) -> Option<f64> {
        self.last_synced.map(|v| v + REFERENCE_EPOCH_OFFSET)
    }
    pub fn is_configured(&self) -> bool {
        self.org_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_wire_values() {
        assert_eq!(serde_json::to_string(&AccountPlan::Max200).unwrap(), "\"Max\"");
        assert_eq!(serde_json::to_string(&AccountPlan::Max5x).unwrap(), "\"Max 5x\"");
        let p: AccountPlan = serde_json::from_str("\"Max 20x\"").unwrap();
        assert!(matches!(p, AccountPlan::Max20x));
    }

    #[test]
    fn source_round_trips_as_manual() {
        let json = r#"{"id":"a","name":"n","chromeProfilePath":"","plan":"Pro",
                       "status":"active","source":"manual"}"#;
        let account = Account::from_json_object(json).unwrap();
        assert_eq!(account.source, AccountSource::Manual);

        let out = serde_json::to_string(&account).unwrap();
        assert!(out.contains(r#""source":"manual""#), "got {out}");
    }

    #[test]
    fn a_record_with_no_source_is_browser_backed() {
        let json = r#"{"id":"a","name":"n","chromeProfilePath":"Default",
                       "plan":"Pro","status":"active"}"#;
        let account = Account::from_json_object(json).unwrap();
        assert_eq!(account.source, AccountSource::Browser);
    }

    #[test]
    fn legacy_json_missing_browser_defaults_to_chrome() {
        // Mirrors AccountCodableTests.testDecodeLegacyJSONDefaultsToChrome.
        let json = r#"{
            "id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B",
            "name":"x","chromeProfilePath":"/p","plan":"Pro","status":"active"
        }"#;
        let a = Account::from_json_object(json).unwrap();
        assert!(matches!(a.browser, Browser::Chrome));
        assert!(!a.is_pinned); // missing isPinned defaults false
    }

    #[test]
    fn uuid_roundtrips_uppercase_hyphenated() {
        let json = r#"{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"x",
            "chromeProfilePath":"/p","plan":"Pro","status":"active"}"#;
        let a = Account::from_json_object(json).unwrap();
        assert_eq!(a.id, "3B8C3678-3A00-425C-8D22-22BCA37AE65B");
        assert!(serde_json::to_string(&a).unwrap()
            .contains("\"id\":\"3B8C3678-3A00-425C-8D22-22BCA37AE65B\""));
    }

    #[test]
    fn last_synced_is_reference_date_double_not_unix() {
        // 2001-01-01T00:00:00Z reference epoch. value 0.0 => unix 978307200.
        let json = r#"{"id":"A","name":"x","chromeProfilePath":"/p","plan":"Pro",
            "status":"active","lastSynced":0.0}"#;
        let a = Account::from_json_object(json).unwrap();
        assert_eq!(a.last_synced_unix().unwrap(), 978307200.0);
    }
}

#[cfg(test)]
mod account_uuid_tests {
    use super::*;

    #[test]
    fn legacy_json_without_account_uuid_decodes() {
        let json = r#"[{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"person@example.com",
            "chromeProfilePath":"Profile 1","orgId":"org-1","plan":"Max","status":"active"}]"#;
        let accounts = Account::from_json(json).expect("decode");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_uuid, None);
    }

    #[test]
    fn account_uuid_round_trips_and_is_omitted_when_absent() {
        let json = r#"[{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"person@example.com",
            "chromeProfilePath":"Profile 1","orgId":"org-1","accountUuid":"acct-1",
            "plan":"Max","status":"active"}]"#;
        let accounts = Account::from_json(json).expect("decode");
        assert_eq!(accounts[0].account_uuid.as_deref(), Some("acct-1"));

        let out = Account::to_json_array(&accounts).expect("encode");
        assert!(out.contains(r#""accountUuid":"acct-1""#), "got {out}");
    }
}
