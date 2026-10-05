//! Job Object command runner: spawn suspended, assign to a KILL_ON_JOB_CLOSE job,
//! resume. Timeout and cancel kill the whole tree; closing the job after the shell
//! exits ends background leftovers so the output pipes always reach EOF.
// Unused by the binary until Task 8 wires it in; Task 8 removes this.
#![allow(dead_code)]

use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use claude_dashboard_core::command_log::{bounded_tail, CommandStatus, MAX_OUTPUT_BYTES};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

use crate::shell::Invocation;

const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub output_tail: String,
    pub started_unix: i64,
    pub finished_unix: i64,
}

struct Job(HANDLE);

// SAFETY: a job handle is a kernel object handle, usable from any thread.
unsafe impl Send for Job {}

impl Job {
    fn new() -> Result<Job, String> {
        // SAFETY: null attributes and name are valid arguments.
        let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if h.is_null() {
            return Err(format!("CreateJobObject failed: {}", std::io::Error::last_os_error()));
        }
        let job = Job(h);
        // SAFETY: all-zero is a valid bit pattern for this plain-data struct.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: handle is live; pointer and size describe `info`.
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(format!("SetInformationJobObject failed: {}", std::io::Error::last_os_error()));
        }
        Ok(job)
    }

    fn assign(&self, process: HANDLE) -> bool {
        // SAFETY: both handles are live for the call.
        unsafe { AssignProcessToJobObject(self.0, process) != 0 }
    }

    fn terminate(&self) {
        if !self.0.is_null() {
            // SAFETY: handle is live (not yet closed).
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    fn close(&mut self) {
        if !self.0.is_null() {
            // SAFETY: closed exactly once, then nulled.
            unsafe { CloseHandle(self.0) };
            self.0 = std::ptr::null_mut();
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.close();
    }
}

/// Resume every thread of the (suspended) process. Returns how many were resumed.
fn resume_process(pid: u32) -> usize {
    let mut resumed = 0;
    // SAFETY: plain snapshot call; result checked below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snap == INVALID_HANDLE_VALUE || snap.is_null() {
        return 0;
    }
    // SAFETY: all-zero is valid for THREADENTRY32; dwSize is set before use.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    // SAFETY: snapshot handle is live and entry is properly sized.
    let mut more = unsafe { Thread32First(snap, &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: opening a thread by id; result checked.
            let th = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !th.is_null() {
                // SAFETY: th is a live thread handle with resume access.
                if unsafe { ResumeThread(th) } != u32::MAX {
                    resumed += 1;
                }
                // SAFETY: th is live and closed once.
                unsafe { CloseHandle(th) };
            }
        }
        // SAFETY: snapshot handle is live and entry is properly sized.
        more = unsafe { Thread32Next(snap, &mut entry) } != 0;
    }
    // SAFETY: snap is live and closed once.
    unsafe { CloseHandle(snap) };
    resumed
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn launch_failed(reason: String, started: i64) -> RunOutcome {
    RunOutcome {
        status: CommandStatus::LaunchFailed,
        exit_code: None,
        output_tail: format!("launch failed: {reason}"),
        started_unix: started,
        finished_unix: now_unix(),
    }
}

fn pump(mut src: impl Read, buf: &Mutex<String>, on_output: &(dyn Fn(&str) + Sync)) {
    let mut chunk = [0u8; 4096];
    loop {
        match src.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let text = String::from_utf8_lossy(&chunk[..n]);
                on_output(&text);
                let mut b = buf.lock().unwrap_or_else(|e| e.into_inner());
                b.push_str(&text);
                let keep = bounded_tail(&b, MAX_OUTPUT_BYTES).len();
                if keep < b.len() {
                    let cut = b.len() - keep;
                    b.drain(..cut);
                }
            }
        }
    }
}

pub fn run(
    inv: &Invocation,
    timeout: Duration,
    cancel: &CancelToken,
    on_output: &(dyn Fn(&str) + Sync),
) -> RunOutcome {
    let started = now_unix();
    let mut job = match Job::new() {
        Ok(j) => j,
        Err(e) => return launch_failed(e, started),
    };

    let mut command = Command::new(&inv.program);
    command.args(&inv.args);
    if let Some(raw) = &inv.raw_args {
        command.raw_arg(raw);
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        command.current_dir(home);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return launch_failed(e.to_string(), started),
    };

    if !job.assign(child.as_raw_handle() as HANDLE) {
        let reason = format!("could not assign to job: {}", std::io::Error::last_os_error());
        let _ = child.kill();
        let _ = child.wait();
        return launch_failed(reason, started);
    }
    if resume_process(child.id()) == 0 {
        let _ = child.kill();
        let _ = child.wait();
        return launch_failed("could not resume the process".into(), started);
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let buf = Mutex::new(String::new());
    let mut status = CommandStatus::Exited;
    let mut code: Option<i32> = None;

    std::thread::scope(|scope| {
        if let Some(s) = stdout {
            scope.spawn(|| pump(s, &buf, on_output));
        }
        if let Some(s) = stderr {
            scope.spawn(|| pump(s, &buf, on_output));
        }

        let deadline = Instant::now() + timeout;
        let mut killed = false;
        loop {
            match child.try_wait() {
                Ok(Some(st)) => {
                    code = st.code();
                    break;
                }
                Ok(None) => {}
                Err(_) => break,
            }
            if !killed {
                if cancel.is_cancelled() {
                    status = CommandStatus::Cancelled;
                    job.terminate();
                    killed = true;
                } else if Instant::now() >= deadline {
                    status = CommandStatus::TimedOut;
                    job.terminate();
                    killed = true;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Ends background leftovers so the readers reach EOF before the scope joins.
        job.close();
    });

    let output_tail = buf.into_inner().unwrap_or_else(|e| e.into_inner());
    RunOutcome {
        status,
        exit_code: if status == CommandStatus::Exited { code } else { None },
        output_tail,
        started_unix: started,
        finished_unix: now_unix(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Instant;

    fn cmd(script: &str) -> Invocation {
        let exe = std::env::var("ComSpec").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".into());
        Invocation { program: PathBuf::from(exe), args: vec!["/d".into(), "/s".into(), "/c".into()],
                     raw_args: Some(format!("\"{script}\"")) }
    }
    fn quiet(_: &str) {}

    #[test]
    fn captures_output_and_exit_zero() {
        let seen = Mutex::new(String::new());
        let out = run(&cmd("echo hello"), Duration::from_secs(20), &CancelToken::new(),
                      &|s| seen.lock().unwrap().push_str(s));
        assert_eq!(out.status, CommandStatus::Exited);
        assert_eq!(out.exit_code, Some(0));
        assert!(out.output_tail.contains("hello"));
        assert!(seen.lock().unwrap().contains("hello"));
        assert!(out.finished_unix >= out.started_unix);
    }

    #[test]
    fn reports_nonzero_exit() {
        let out = run(&cmd("exit 3"), Duration::from_secs(20), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::Exited);
        assert_eq!(out.exit_code, Some(3));
    }

    #[test]
    fn timeout_kills_tree() {
        let t = Instant::now();
        let out = run(&cmd("ping -n 30 127.0.0.1 > nul"), Duration::from_secs(1), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::TimedOut);
        assert_eq!(out.exit_code, None);
        assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    }

    #[test]
    fn cancel_kills_tree() {
        let token = CancelToken::new();
        let c = token.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(300)); c.cancel(); });
        let t = Instant::now();
        let out = run(&cmd("ping -n 30 127.0.0.1"), Duration::from_secs(60), &token, &quiet);
        assert_eq!(out.status, CommandStatus::Cancelled);
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn background_grandchild_does_not_hang() {
        // cmd exits at once; the ping inherits the output pipe. Without the job
        // closing, the readers would block ~30s.
        let t = Instant::now();
        let out = run(&cmd("start /b ping -n 30 127.0.0.1"), Duration::from_secs(60), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::Exited);
        assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    }

    #[test]
    fn stdin_is_null_so_reads_return() {
        let t = Instant::now();
        let out = run(&cmd("set /p x=& echo after"), Duration::from_secs(20), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::Exited);
        assert!(out.output_tail.contains("after"));
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn chatty_output_is_bounded() {
        let out = run(&cmd("for /l %i in (1,1,3000) do @echo line%i"), Duration::from_secs(60),
                      &CancelToken::new(), &quiet);
        assert!(out.output_tail.len() <= MAX_OUTPUT_BYTES);
        assert!(out.output_tail.trim_end().ends_with("line3000"));
    }

    #[test]
    fn missing_program_is_launch_failed() {
        let inv = Invocation { program: PathBuf::from("C:\\definitely\\missing.exe"), args: vec![], raw_args: None };
        let out = run(&inv, Duration::from_secs(5), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::LaunchFailed);
        assert!(out.output_tail.starts_with("launch failed"));
    }
}
