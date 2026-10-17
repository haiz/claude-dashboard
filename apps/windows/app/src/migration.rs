//! One-shot launch cleanup: removes the files the retired Run Command feature
//! left behind. Every error is ignored; this must never block startup.

use std::path::Path;

const APP_DIR: &str = "claude-dashboard";

/// Deletes `<local>\claude-dashboard\command_logs.db` (plus `-wal`/`-shm`) and
/// `<appdata>\claude-dashboard\run-commands.json`. Missing files are fine.
pub fn run_launch_cleanup(local_app_data: &Path, app_data: &Path) {
    let local = local_app_data.join(APP_DIR);
    for name in ["command_logs.db", "command_logs.db-wal", "command_logs.db-shm"] {
        let _ = std::fs::remove_file(local.join(name));
    }
    let _ = std::fs::remove_file(app_data.join(APP_DIR).join("run-commands.json"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn deletes_all_leftovers() {
        let local = tempfile::tempdir().unwrap();
        let roam = tempfile::tempdir().unwrap();
        let files = [
            local.path().join("claude-dashboard/command_logs.db"),
            local.path().join("claude-dashboard/command_logs.db-wal"),
            local.path().join("claude-dashboard/command_logs.db-shm"),
            roam.path().join("claude-dashboard/run-commands.json"),
        ];
        files.iter().for_each(|f| touch(f));
        run_launch_cleanup(local.path(), roam.path());
        assert!(files.iter().all(|f| !f.exists()));
    }

    #[test]
    fn missing_files_are_fine() {
        let local = tempfile::tempdir().unwrap();
        let roam = tempfile::tempdir().unwrap();
        run_launch_cleanup(local.path(), roam.path());
        run_launch_cleanup(&local.path().join("nope"), &roam.path().join("nope"));
    }

    #[test]
    fn unrelated_files_survive() {
        let local = tempfile::tempdir().unwrap();
        let roam = tempfile::tempdir().unwrap();
        let keep = [
            local.path().join("claude-dashboard/usage_logs.db"),
            roam.path().join("claude-dashboard/accounts.json"),
        ];
        keep.iter().for_each(|f| touch(f));
        run_launch_cleanup(local.path(), roam.path());
        assert!(keep.iter().all(|f| f.exists()));
    }
}
