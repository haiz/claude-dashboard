//! Single-instance guard: a named mutex held for the process lifetime.

/// Held while this process is the running instance; released on drop.
pub struct SingleInstance {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

const MUTEX_NAME: &str = "Local\\ClaudeDashboardSingleInstance";

/// `Some` if this process is the first instance, `None` if one is running.
pub fn acquire_single_instance() -> Option<SingleInstance> {
    acquire_named(MUTEX_NAME)
}

#[cfg(windows)]
fn acquire_named(name: &str) -> Option<SingleInstance> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return None;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return None;
    }
    Some(SingleInstance { handle })
}

#[cfg(not(windows))]
fn acquire_named(_name: &str) -> Option<SingleInstance> {
    Some(SingleInstance {})
}

#[cfg(windows)]
impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn single_instance_guard_rejects_the_second_holder() {
        let name = format!("Local\\ClaudeDashboardTest{}", std::process::id());
        let first = acquire_named(&name);
        assert!(first.is_some());
        assert!(acquire_named(&name).is_none());
        drop(first);
        assert!(acquire_named(&name).is_some());
    }
}
