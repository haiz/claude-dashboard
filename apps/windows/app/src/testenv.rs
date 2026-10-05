//! Process-wide env mutex for tests that point APPDATA/LOCALAPPDATA at a
//! tempdir; env vars are shared across the test threads of this crate.
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}
