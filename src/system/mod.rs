use std::{
    io,
    process::{Child, Command, Stdio},
    sync::Arc,
};

#[cfg(target_os = "windows")]
use std::path::PathBuf;

pub mod dns;
pub mod proxy;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub(crate) mod resolved;

#[cfg(target_os = "linux")]
pub(crate) mod resolvconf;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "windows")]
pub(crate) mod powershell_dns;

/// Proxy configuration is handled separately by [`proxy::ProxyBackend`].
pub trait SystemServices: Send + Sync {
    fn spawn_client(
        &self,
        binary: &str,
        configuration_path: &std::path::Path,
        needs_elevation: bool,
    ) -> io::Result<ChildProcess>;

    fn terminate_process(&self, process_id: u32) -> bool;

    fn elevate_terminate_process(&self, process_id: u32) -> bool;

    fn find_client_binary(&self) -> (String, bool);

    fn check_tun_device(&self) -> bool;

    fn check_elevation_available(&self) -> bool;

    fn check_binary_works(&self, binary: &str, needs_root: bool) -> Option<String> {
        check_binary_works(binary, needs_root)
    }

    fn startup_cleanup(&self) {}

    fn emergency_cleanup(&self) {
        proxy::emergency_clear();
        dns::emergency_clear();
    }
}

#[cfg(target_os = "linux")]
pub struct LinuxSystem;

#[cfg(target_os = "windows")]
pub struct WindowsSystem;

#[cfg(target_os = "linux")]
pub fn system_services() -> Arc<dyn SystemServices> {
    Arc::new(LinuxSystem)
}

#[cfg(target_os = "windows")]
pub fn system_services() -> Arc<dyn SystemServices> {
    Arc::new(WindowsSystem)
}

#[cfg(target_os = "linux")]
impl SystemServices for LinuxSystem {
    fn spawn_client(
        &self,
        binary: &str,
        configuration_path: &std::path::Path,
        needs_elevation: bool,
    ) -> io::Result<ChildProcess> {
        linux::spawn_client(binary, configuration_path, needs_elevation)
    }

    fn terminate_process(&self, process_id: u32) -> bool {
        linux::terminate_process(process_id)
    }

    fn elevate_terminate_process(&self, process_id: u32) -> bool {
        linux::elevate_terminate_process(process_id)
    }

    fn find_client_binary(&self) -> (String, bool) {
        linux::find_client_binary()
    }

    fn check_tun_device(&self) -> bool {
        linux::check_tun_device()
    }

    fn check_elevation_available(&self) -> bool {
        linux::check_elevation_available()
    }
}

#[cfg(target_os = "windows")]
impl SystemServices for WindowsSystem {
    fn spawn_client(
        &self,
        binary: &str,
        configuration_path: &std::path::Path,
        needs_elevation: bool,
    ) -> io::Result<ChildProcess> {
        windows::spawn_client(binary, configuration_path, needs_elevation)
    }

    fn terminate_process(&self, process_id: u32) -> bool {
        windows::terminate_process(process_id)
    }

    fn elevate_terminate_process(&self, process_id: u32) -> bool {
        windows::elevate_terminate_process(process_id)
    }

    fn find_client_binary(&self) -> (String, bool) {
        windows::find_client_binary()
    }

    fn check_tun_device(&self) -> bool {
        windows::check_tun_device()
    }

    fn check_elevation_available(&self) -> bool {
        windows::check_elevation_available()
    }

    fn startup_cleanup(&self) {
        windows::install_ctrl_handler();
        windows::cleanup_stale_system_proxy();
        windows::cleanup_stale_elevated_files();
    }

    fn emergency_cleanup(&self) {
        proxy::emergency_clear();
        dns::emergency_clear();
        windows::terminate_elevated_client();
        windows::cleanup_elevated_files();
    }
}

pub struct ChildExit {
    pub code: Option<i32>,
}

impl ChildExit {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

impl std::fmt::Display for ChildExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(code) => write!(f, "exit code: {code}"),
            None => write!(f, "terminated by signal"),
        }
    }
}

pub enum ChildProcess {
    Direct {
        child: Child,
        #[cfg(target_os = "windows")]
        _job_guard: Option<JobGuard>,
    },
    #[cfg(target_os = "windows")]
    Elevated {
        log_path: PathBuf,
        exit_marker_path: PathBuf,
    },
}

impl ChildProcess {
    pub fn id(&self) -> Option<u32> {
        match self {
            Self::Direct { child, .. } => Some(child.id()),
            #[cfg(target_os = "windows")]
            Self::Elevated { .. } => None,
        }
    }

    pub fn is_elevated(&self) -> bool {
        match self {
            Self::Direct { .. } => false,
            #[cfg(target_os = "windows")]
            Self::Elevated { .. } => true,
        }
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ChildExit>> {
        match self {
            Self::Direct { child, .. } => child
                .try_wait()
                .map(|opt| opt.map(|s| ChildExit { code: s.code() })),
            #[cfg(target_os = "windows")]
            Self::Elevated {
                exit_marker_path, ..
            } => {
                if exit_marker_path.exists() {
                    let content = std::fs::read_to_string(exit_marker_path).unwrap_or_default();
                    let code = content.trim().parse::<i32>().ok();
                    Ok(Some(ChildExit { code }))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub fn kill(&mut self) {
        match self {
            Self::Direct { child, .. } => {
                let _ = child.kill();
            }
            #[cfg(target_os = "windows")]
            Self::Elevated { .. } => {
                terminate_elevated_client();
            }
        }
    }

    pub fn wait(&mut self) -> ChildExit {
        match self {
            Self::Direct { child, .. } => {
                let status = child.wait();
                ChildExit {
                    code: status.ok().and_then(|s| s.code()),
                }
            }
            #[cfg(target_os = "windows")]
            Self::Elevated {
                exit_marker_path, ..
            } => loop {
                if exit_marker_path.exists() {
                    let content = std::fs::read_to_string(exit_marker_path).unwrap_or_default();
                    return ChildExit {
                        code: content.trim().parse::<i32>().ok(),
                    };
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            },
        }
    }

    pub fn take_stderr(&mut self) -> Option<Box<dyn io::Read + Send>> {
        match self {
            Self::Direct { child, .. } => child
                .stderr
                .take()
                .map(|r| Box::new(r) as Box<dyn io::Read + Send>),
            #[cfg(target_os = "windows")]
            Self::Elevated { .. } => None,
        }
    }

    pub fn take_stdout(&mut self) -> Option<Box<dyn io::Read + Send>> {
        match self {
            Self::Direct { child, .. } => child
                .stdout
                .take()
                .map(|r| Box::new(r) as Box<dyn io::Read + Send>),
            #[cfg(target_os = "windows")]
            Self::Elevated { .. } => None,
        }
    }

    #[cfg(target_os = "windows")]
    pub fn elevated_log_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Elevated { log_path, .. } => Some(log_path),
            Self::Direct { .. } => None,
        }
    }

    #[cfg(target_os = "windows")]
    pub fn elevated_exit_marker_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Elevated {
                exit_marker_path, ..
            } => Some(exit_marker_path),
            Self::Direct { .. } => None,
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn run_silent_with_output(program: &str, arguments: &[&str]) -> (bool, String) {
    log::debug!("[cmd] {} {}", program, arguments.join(" "));
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    match command.output() {
        Ok(output) => {
            let success = output.status.success();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if !success {
                log::debug!(
                    "[cmd] FAILED (exit {}): {} {}\n  stdout: {}\n  stderr: {}",
                    output.status.code().unwrap_or(-1),
                    program,
                    arguments.join(" "),
                    stdout.trim(),
                    stderr.trim(),
                );
            } else {
                log::trace!(
                    "[cmd] OK: {} {} → stdout={}",
                    program,
                    arguments.join(" "),
                    stdout.trim(),
                );
            }
            (success, stdout)
        }
        Err(error) => {
            log::debug!("[cmd] spawn error for {}: {}", program, error);
            (false, error.to_string())
        }
    }
}

pub fn run_silent(program: &str, arguments: &[&str]) -> bool {
    run_silent_with_output(program, arguments).0
}

pub fn check_binary_works(binary: &str, needs_root: bool) -> Option<String> {
    if needs_root {
        log::debug!("[preflight] skipping elevated binary check (would prompt for auth)");
        return None;
    }

    log::debug!("[preflight] testing binary: {binary} --help");

    let mut command = Command::new(binary);
    command
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.is_empty() || !stderr.is_empty() {
                let first_line = stdout
                    .lines()
                    .chain(stderr.lines())
                    .next()
                    .unwrap_or("(no output)");
                log::info!("[preflight] binary OK: {first_line}");
                None
            } else {
                log::warn!("[preflight] binary produced no output");
                None
            }
        }
        Err(error) => {
            let message = format!("Cannot run '{binary}': {error}");
            log::error!("[preflight] {message}");
            Some(message)
        }
    }
}

pub fn parse_host_port(address: &str) -> (String, u16) {
    // Handle [IPv6]:port bracket notation (e.g. "[::1]:1080")
    if address.starts_with('[')
        && let Some(bracket_end) = address.find(']')
    {
        let host = &address[1..bracket_end];
        let port = address[bracket_end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(1080);
        return (host.to_string(), port);
    }

    if let Some(index) = address.rfind(':') {
        let before_colon = &address[..index];
        // If there is another colon before this one it is a bare IPv6 address
        // without an explicit port (e.g. "::1" or "2001:db8::1").
        if before_colon.contains(':') {
            return (address.to_string(), 1080);
        }
        let port = address[index + 1..].parse::<u16>().unwrap_or(1080);
        (before_colon.to_string(), port)
    } else {
        (address.to_string(), 1080)
    }
}
