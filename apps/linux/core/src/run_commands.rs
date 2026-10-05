//! Per-account saved run command (macOS `RunCommandSettings`): one command and
//! its "open in terminal" choice per `Account.id`. Deleting the account drops
//! it; a re-sync keeps the id, so it survives that.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

const FORMAT_VERSION: u32 = 1;
static RMW: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunCommand {
    pub command: String,
    #[serde(rename = "openInTerminal", default)]
    pub open_in_terminal: bool,
}

#[derive(Serialize, Deserialize)]
struct FileShape {
    version: u32,
    #[serde(default)]
    commands: HashMap<String, RunCommand>,
}

pub fn run_commands_path() -> PathBuf {
    crate::store::accounts_path().with_file_name("run-commands.json")
}

pub fn load_at(path: &Path) -> HashMap<String, RunCommand> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<FileShape>(&t).ok())
        .filter(|f| f.version == FORMAT_VERSION)
        .map(|f| f.commands)
        .unwrap_or_default()
}

fn save_at(path: &Path, commands: HashMap<String, RunCommand>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&FileShape { version: FORMAT_VERSION, commands })
        .map_err(|e| e.to_string())?;
    crate::settings::write_atomic(path, json.as_bytes())
}

fn modify_at(path: &Path, f: impl FnOnce(&mut HashMap<String, RunCommand>)) -> Result<(), String> {
    let _g = RMW.lock().unwrap_or_else(|p| p.into_inner());
    let mut m = load_at(path);
    f(&mut m);
    save_at(path, m)
}

pub fn set_at(path: &Path, account_id: &str, rc: &RunCommand) -> Result<(), String> {
    modify_at(path, |m| {
        if rc.command.trim().is_empty() {
            m.remove(account_id);
        } else {
            m.insert(account_id.to_string(), rc.clone());
        }
    })
}

pub fn remove_at(path: &Path, account_id: &str) -> Result<(), String> {
    modify_at(path, |m| {
        m.remove(account_id);
    })
}

pub fn load() -> HashMap<String, RunCommand> {
    load_at(&run_commands_path())
}
pub fn get(account_id: &str) -> Option<RunCommand> {
    load().remove(account_id)
}
pub fn set(account_id: &str, rc: &RunCommand) -> Result<(), String> {
    set_at(&run_commands_path(), account_id, rc)
}
pub fn remove(account_id: &str) -> Result<(), String> {
    remove_at(&run_commands_path(), account_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rc(cmd: &str, term: bool) -> RunCommand {
        RunCommand { command: cmd.into(), open_in_terminal: term }
    }

    #[test]
    fn missing_or_corrupt_loads_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        assert!(load_at(&p).is_empty());
        std::fs::write(&p, "{ nope").unwrap();
        assert!(load_at(&p).is_empty());
        std::fs::write(&p, r#"{"version":2,"commands":{"a":{"command":"x"}}}"#).unwrap();
        assert!(load_at(&p).is_empty(), "unknown version ignored");
    }

    #[test]
    fn set_get_roundtrip_and_independent_keys() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nested").join("run-commands.json");
        set_at(&p, "a", &rc("ccbf", true)).unwrap();
        set_at(&p, "b", &rc("echo hi", false)).unwrap();
        let m = load_at(&p);
        assert_eq!(m.get("a"), Some(&rc("ccbf", true)));
        assert_eq!(m.get("b"), Some(&rc("echo hi", false)));
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1, "no temp litter");
    }

    #[test]
    fn blank_command_removes_entry() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        set_at(&p, "a", &rc("ccbf", false)).unwrap();
        set_at(&p, "a", &rc("   ", false)).unwrap();
        assert!(!load_at(&p).contains_key("a"));
    }

    #[test]
    fn remove_drops_only_that_account() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        set_at(&p, "a", &rc("x", false)).unwrap();
        set_at(&p, "b", &rc("y", false)).unwrap();
        remove_at(&p, "a").unwrap();
        remove_at(&p, "missing").unwrap();
        let m = load_at(&p);
        assert!(!m.contains_key("a"));
        assert!(m.contains_key("b"));
    }

    #[test]
    fn open_in_terminal_defaults_false() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        std::fs::write(&p, r#"{"version":1,"commands":{"a":{"command":"x"}}}"#).unwrap();
        assert_eq!(load_at(&p).get("a"), Some(&rc("x", false)));
    }

    #[test]
    fn path_is_beside_accounts() {
        assert_eq!(run_commands_path().file_name().unwrap(), "run-commands.json");
        assert_eq!(run_commands_path().parent(), crate::store::accounts_path().parent());
    }
}
