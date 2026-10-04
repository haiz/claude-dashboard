//! Launch-at-startup via the per-user Run key (Windows).

/// The Run value data: the exe path wrapped in double quotes, so a path
/// containing spaces is a single argument.
pub fn startup_command(exe_path: &str) -> String {
    format!("\"{exe_path}\"")
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "ClaudeDashboard";

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn enable_named(value_name: &str, exe_path: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let key = wide(RUN_KEY);
    let name = wide(value_name);
    let data = wide(&startup_command(exe_path));
    let cb = (data.len() * 2) as u32;
    // SAFETY: all pointers reference live NUL-terminated UTF-16 buffers; cb
    // is the byte length of `data` including the terminator.
    let rc = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            cb,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("RegSetKeyValueW failed: {rc}"))
    }
}

#[cfg(windows)]
fn disable_named(value_name: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
    let key = wide(RUN_KEY);
    let name = wide(value_name);
    // SAFETY: both pointers reference live NUL-terminated UTF-16 buffers.
    let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    if rc == 0 || rc == ERROR_FILE_NOT_FOUND || rc == ERROR_PATH_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("RegDeleteKeyValueW failed: {rc}"))
    }
}

/// Reads the REG_SZ value, or None when absent/not a string.
#[cfg(windows)]
fn read_named(value_name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let key = wide(RUN_KEY);
    let name = wide(value_name);
    let mut buf = [0u16; 2048];
    let mut cb = std::mem::size_of_val(&buf) as u32;
    // SAFETY: buf is valid for cb bytes; pointers are NUL-terminated wide strings.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut cb,
        )
    };
    if rc != 0 {
        return None;
    }
    let units = (cb as usize / 2).min(buf.len());
    let s = &buf[..units];
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    Some(String::from_utf16_lossy(&s[..end]))
}

#[cfg(windows)]
fn is_enabled_named(value_name: &str) -> bool {
    read_named(value_name).is_some()
}

/// Register the app to launch at login.
#[cfg(windows)]
pub fn enable(exe_path: &str) -> Result<(), String> {
    enable_named(VALUE_NAME, exe_path)
}

/// Remove the launch-at-login entry (absent is Ok).
#[cfg(windows)]
pub fn disable() -> Result<(), String> {
    disable_named(VALUE_NAME)
}

/// Whether the launch-at-login entry is present.
#[cfg(windows)]
pub fn is_enabled() -> bool {
    is_enabled_named(VALUE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_is_quoted() {
        assert_eq!(
            startup_command(r"C:\Program Files\x\claude-dashboard.exe"),
            "\"C:\\Program Files\\x\\claude-dashboard.exe\""
        );
    }

    #[cfg(windows)]
    #[test]
    fn startup_value_roundtrips() {
        let name = format!("ClaudeDashboardTest{}", std::process::id());
        let exe = r"C:\Program Files\x\claude-dashboard.exe";
        assert!(!is_enabled_named(&name));
        enable_named(&name, exe).unwrap();
        assert!(is_enabled_named(&name));
        assert_eq!(read_named(&name).unwrap(), startup_command(exe));
        disable_named(&name).unwrap();
        assert!(!is_enabled_named(&name));
        disable_named(&name).unwrap();
    }
}
