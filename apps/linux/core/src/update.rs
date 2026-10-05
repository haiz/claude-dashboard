//! Release-check decisions (ports UpdateService.swift / apps/linux/lib/update.js).
//! Pure except `fetch_latest_release`; the download and install live in the app.

use serde_json::Value;

pub const REPO_SLUG: &str = "haiz/claude-dashboard";
pub const RELEASES_URL: &str = "https://api.github.com/repos/haiz/claude-dashboard/releases/latest";
pub const MSI_ASSET: &str = "ClaudeDashboard-x64.msi";
pub const CHECK_INTERVAL_S: f64 = 86400.0;

fn parts(v: &str) -> Vec<u64> {
    v.split('.').filter_map(|p| p.parse::<u64>().ok()).collect()
}

/// Component-wise numeric compare; a missing component reads as 0 and a
/// non-numeric one is dropped (UpdateService.isNewer's compactMap).
pub fn is_newer(remote: &str, current: &str) -> bool {
    let (r, c) = (parts(remote), parts(current));
    for i in 0..r.len().max(c.len()) {
        let (rv, cv) = (r.get(i).copied().unwrap_or(0), c.get(i).copied().unwrap_or(0));
        if rv != cv {
            return rv > cv;
        }
    }
    false
}

pub fn version_from_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub download_url: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateError {
    NotARelease,
    AssetNotFound,
    Http(String),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::NotARelease => write!(f, "Unexpected response from GitHub."),
            UpdateError::AssetNotFound => write!(f, "The release has no Windows installer."),
            UpdateError::Http(m) => write!(f, "{m}"),
        }
    }
}

/// `Ok(None)` = up to date (or a draft/prerelease, which `releases/latest`
/// should never return but is ignored defensively).
pub fn parse_release(json: &str, current: &str, asset: &str) -> Result<Option<UpdateInfo>, UpdateError> {
    let v: Value = serde_json::from_str(json).map_err(|_| UpdateError::NotARelease)?;
    let tag = v.get("tag_name").and_then(Value::as_str).ok_or(UpdateError::NotARelease)?;
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return Ok(None);
    }
    let version = version_from_tag(tag);
    if !is_newer(version, current) {
        return Ok(None);
    }
    let url = v
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|x| x.get("name").and_then(Value::as_str) == Some(asset)))
        .and_then(|x| x.get("browser_download_url").and_then(Value::as_str))
        .ok_or(UpdateError::AssetNotFound)?;
    Ok(Some(UpdateInfo {
        version: version.to_string(),
        download_url: url.to_string(),
        body: v.get("body").and_then(Value::as_str).map(str::to_string),
    }))
}

/// Never checked, or `interval_s` has passed, or the clock moved backwards.
pub fn is_due(last_unix: Option<f64>, now_unix: f64, interval_s: f64) -> bool {
    match last_unix {
        None => true,
        Some(l) if l <= 0.0 => true,
        Some(l) => now_unix < l || now_unix - l >= interval_s,
    }
}

/// GET `releases/latest`. Non-200 and transport failures become `Http`.
pub fn fetch_latest_release(current_version: &str) -> Result<String, UpdateError> {
    let resp = ureq::get(RELEASES_URL)
        .timeout(std::time::Duration::from_secs(20))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .set("User-Agent", &format!("claude-dashboard/{current_version}"))
        .call()
        .map_err(|e| UpdateError::Http(format!("Update check failed: {e}")))?;
    if resp.status() != 200 {
        return Err(UpdateError::Http(format!("Update check failed: HTTP {}", resp.status())));
    }
    resp.into_string().map_err(|e| UpdateError::Http(format!("Update check failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_compares_numerically() {
        assert!(is_newer("1.19.0", "1.18.1"));
        assert!(is_newer("1.18.10", "1.18.9"), "numeric, not lexical");
        assert!(is_newer("2.0", "1.99.99"));
        assert!(!is_newer("1.18.1", "1.18.1"));
        assert!(!is_newer("1.18.0", "1.18.1"), "never downgrade");
        assert!(!is_newer("1.18", "1.18.0"), "missing component is 0");
        assert!(is_newer("1.18.0.1", "1.18"));
        assert!(!is_newer("", "1.0.0"));
        assert!(!is_newer("garbage", "1.0.0"));
    }

    #[test]
    fn tag_prefix_is_stripped_once() {
        assert_eq!(version_from_tag("v1.2.3"), "1.2.3");
        assert_eq!(version_from_tag("1.2.3"), "1.2.3");
        assert_eq!(version_from_tag("vv1"), "v1");
    }

    fn payload(tag: &str, extra: &str, assets: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","body":"notes","html_url":"https://x"{extra},"assets":[{assets}]}}"#)
    }
    const MSI: &str = r#"{"name":"ClaudeDashboard-x64.msi","browser_download_url":"https://dl/msi"}"#;
    const ZIP: &str = r#"{"name":"ClaudeDashboard.app.zip","browser_download_url":"https://dl/zip"}"#;

    #[test]
    fn parse_release_returns_newer_msi() {
        let got = parse_release(&payload("v1.19.0", "", &format!("{ZIP},{MSI}")), "1.18.1", MSI_ASSET).unwrap();
        assert_eq!(got, Some(UpdateInfo { version: "1.19.0".into(), download_url: "https://dl/msi".into(), body: Some("notes".into()) }));
    }

    #[test]
    fn parse_release_none_when_not_newer() {
        assert_eq!(parse_release(&payload("v1.18.1", "", MSI), "1.18.1", MSI_ASSET).unwrap(), None);
        assert_eq!(parse_release(&payload("v1.0.0", "", MSI), "1.18.1", MSI_ASSET).unwrap(), None);
    }

    #[test]
    fn parse_release_ignores_draft_and_prerelease() {
        assert_eq!(parse_release(&payload("v9.0.0", r#","draft":true"#, MSI), "1.0.0", MSI_ASSET).unwrap(), None);
        assert_eq!(parse_release(&payload("v9.0.0", r#","prerelease":true"#, MSI), "1.0.0", MSI_ASSET).unwrap(), None);
    }

    #[test]
    fn parse_release_newer_without_msi_is_asset_not_found() {
        assert_eq!(parse_release(&payload("v9.0.0", "", ZIP), "1.0.0", MSI_ASSET), Err(UpdateError::AssetNotFound));
    }

    #[test]
    fn parse_release_rejects_non_release() {
        for bad in ["", "{", "[]", r#"{"message":"Not Found"}"#, r#"{"tag_name":5}"#] {
            assert_eq!(parse_release(bad, "1.0.0", MSI_ASSET), Err(UpdateError::NotARelease), "{bad:?}");
        }
    }

    #[test]
    fn is_due_respects_interval() {
        assert!(is_due(None, 1000.0, CHECK_INTERVAL_S));
        assert!(is_due(Some(0.0), 1000.0, CHECK_INTERVAL_S), "0 means never");
        assert!(!is_due(Some(1000.0), 1000.0 + 3600.0, CHECK_INTERVAL_S));
        assert!(is_due(Some(1000.0), 1000.0 + CHECK_INTERVAL_S, CHECK_INTERVAL_S));
        assert!(is_due(Some(5000.0), 1000.0, CHECK_INTERVAL_S), "clock went backwards -> check");
    }
}
