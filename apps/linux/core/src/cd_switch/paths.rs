//! Where Claude Desktop keeps its data on Windows: the Squirrel/exe install under
//! `%APPDATA%\Claude` and the MSIX package under
//! `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude`. A directory only counts
//! when it holds `Local State`; with both present the one with the newer `config.json` wins.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Filesystem reads the resolver needs, injectable for tests.
pub trait PathProbe {
    fn exists(&self, p: &Path) -> bool;
    fn modified(&self, p: &Path) -> Option<SystemTime>;
    fn list_dir(&self, p: &Path) -> Vec<PathBuf>;
}

pub struct StdProbe;

impl PathProbe for StdProbe {
    fn exists(&self, p: &Path) -> bool {
        p.exists()
    }
    fn modified(&self, p: &Path) -> Option<SystemTime> {
        std::fs::metadata(p).and_then(|m| m.modified()).ok()
    }
    fn list_dir(&self, p: &Path) -> Vec<PathBuf> {
        match std::fs::read_dir(p) {
            Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPaths {
    pub data_dir: PathBuf,
    pub local_state: PathBuf,
    pub config: PathBuf,
    pub cookies: PathBuf,
}

fn candidate(dir: PathBuf, fs: &dyn PathProbe) -> Option<DesktopPaths> {
    let local_state = dir.join("Local State");
    if !fs.exists(&local_state) {
        return None;
    }
    let network = dir.join("Network").join("Cookies");
    let cookies = if fs.exists(&network) { network } else { dir.join("Cookies") };
    Some(DesktopPaths { config: dir.join("config.json"), data_dir: dir, local_state, cookies })
}

pub fn resolve(
    appdata: Option<&Path>,
    localappdata: Option<&Path>,
    fs: &dyn PathProbe,
) -> Option<DesktopPaths> {
    let squirrel = appdata.and_then(|a| candidate(a.join("Claude"), fs));
    let msix = localappdata.and_then(|l| {
        let mut pkgs: Vec<PathBuf> = fs
            .list_dir(&l.join("Packages"))
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_ascii_lowercase().starts_with("claude_"))
            })
            .collect();
        pkgs.sort();
        pkgs.into_iter().find_map(|p| {
            candidate(p.join("LocalCache").join("Roaming").join("Claude"), fs)
        })
    });
    match (squirrel, msix) {
        (Some(s), Some(m)) => {
            let ts = fs.modified(&s.config);
            let tm = fs.modified(&m.config);
            // Newest config wins; no config on either side, or a tie, prefers MSIX.
            if ts > tm { Some(s) } else { Some(m) }
        }
        (s, m) => s.or(m),
    }
}

/// Resolves from the real environment (`APPDATA`, `LOCALAPPDATA`).
pub fn live() -> Option<DesktopPaths> {
    let a = std::env::var_os("APPDATA").map(PathBuf::from);
    let l = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    resolve(a.as_deref(), l.as_deref(), &StdProbe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    #[derive(Default)]
    struct Fake {
        files: HashMap<PathBuf, SystemTime>,
    }
    impl Fake {
        fn add(&mut self, p: &str, secs: u64) {
            self.files.insert(PathBuf::from(p), SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        }
    }
    impl PathProbe for Fake {
        fn exists(&self, p: &Path) -> bool {
            self.files.contains_key(p)
        }
        fn modified(&self, p: &Path) -> Option<SystemTime> {
            self.files.get(p).copied()
        }
        fn list_dir(&self, p: &Path) -> Vec<PathBuf> {
            let mut out: Vec<PathBuf> = Vec::new();
            for f in self.files.keys() {
                if let Ok(rest) = f.strip_prefix(p) {
                    if let Some(first) = rest.components().next() {
                        let c = p.join(first);
                        if !out.contains(&c) {
                            out.push(c);
                        }
                    }
                }
            }
            out
        }
    }

    const A: &str = "/ad";
    const L: &str = "/lad";
    const SQ: &str = "/ad/Claude";
    const MS: &str = "/lad/Packages/Claude_pzs8sxrjxfjjc/LocalCache/Roaming/Claude";

    fn run(f: &Fake) -> Option<DesktopPaths> {
        resolve(Some(Path::new(A)), Some(Path::new(L)), f)
    }

    #[test]
    fn squirrel_only() {
        let mut f = Fake::default();
        f.add(&format!("{SQ}/Local State"), 1);
        let p = run(&f).unwrap();
        assert_eq!(p.data_dir, PathBuf::from(SQ));
        assert_eq!(p.config, PathBuf::from(SQ).join("config.json"));
        assert_eq!(p.cookies, PathBuf::from(SQ).join("Cookies"));
        f.add(&format!("{SQ}/Network/Cookies"), 1);
        assert_eq!(run(&f).unwrap().cookies, PathBuf::from(SQ).join("Network").join("Cookies"));
    }

    #[test]
    fn msix_only() {
        let mut f = Fake::default();
        f.add(&format!("{MS}/Local State"), 1);
        assert_eq!(run(&f).unwrap().data_dir, PathBuf::from(MS));
    }

    #[test]
    fn both_picks_newest_config() {
        let mut f = Fake::default();
        f.add(&format!("{SQ}/Local State"), 1);
        f.add(&format!("{MS}/Local State"), 1);
        f.add(&format!("{SQ}/config.json"), 20);
        f.add(&format!("{MS}/config.json"), 10);
        assert_eq!(run(&f).unwrap().data_dir, PathBuf::from(SQ));
        f.add(&format!("{MS}/config.json"), 30);
        assert_eq!(run(&f).unwrap().data_dir, PathBuf::from(MS));
    }

    #[test]
    fn both_without_config_prefers_msix() {
        let mut f = Fake::default();
        f.add(&format!("{SQ}/Local State"), 1);
        f.add(&format!("{MS}/Local State"), 1);
        assert_eq!(run(&f).unwrap().data_dir, PathBuf::from(MS));
    }

    #[test]
    fn dir_without_local_state_is_ignored() {
        let mut f = Fake::default();
        f.add(&format!("{SQ}/config.json"), 99);
        f.add(&format!("{MS}/Local State"), 1);
        assert_eq!(run(&f).unwrap().data_dir, PathBuf::from(MS));
    }

    #[test]
    fn none_found() {
        assert!(run(&Fake::default()).is_none());
        assert!(resolve(None, None, &Fake::default()).is_none());
    }
}
