use std::{
    io,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use super::{CREATE_NO_WINDOW, ChildProcess, proxy::ProxyBackend, run_silent};

const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

mod win32 {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                    SetInformationJobObject,
                },
                Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
            },
            UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        },
        core::PCWSTR,
    };

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn terminate(process_id: u32, exit_code: u32) -> bool {
        unsafe {
            let handle = match OpenProcess(PROCESS_TERMINATE, false, process_id) {
                Ok(handle) => handle,
                Err(error) => {
                    log::warn!(
                        "[win32] OpenProcess failed for pid {}: {}",
                        process_id,
                        error,
                    );
                    return false;
                }
            };
            let result = TerminateProcess(handle, exit_code);
            let _ = CloseHandle(handle);
            if let Err(error) = result {
                log::warn!(
                    "[win32] TerminateProcess failed for pid {}: {}",
                    process_id,
                    error,
                );
                return false;
            }
            true
        }
    }

    pub fn shell_execute_runas_with_args(file: &str, args: Option<&str>) -> bool {
        let verb = to_wide("runas");
        let file_wide = to_wide(file);
        let args_wide = args.map(to_wide);

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file_wide.as_ptr()),
            lpParameters: args_wide
                .as_ref()
                .map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr())),
            nShow: 0, // SW_HIDE
            ..Default::default()
        };

        let result = unsafe { ShellExecuteExW(&mut info) };
        if let Err(error) = result {
            log::warn!("[win32] ShellExecuteExW failed: {error}");
            return false;
        }
        if !info.hProcess.0.is_null() {
            unsafe {
                let _ = CloseHandle(info.hProcess);
            }
        }
        true
    }

    /// All assigned processes die when the last handle closes (even on crash).
    pub fn create_kill_on_close_job() -> Option<HANDLE> {
        unsafe {
            let job = match CreateJobObjectW(None, None) {
                Ok(job) => job,
                Err(error) => {
                    log::warn!("[win32] CreateJobObjectW failed: {error}");
                    return None;
                }
            };

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let result = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if let Err(error) = result {
                log::warn!("[win32] SetInformationJobObject failed: {error}");
                let _ = CloseHandle(job);
                return None;
            }

            Some(job)
        }
    }

    pub fn assign_process_to_job(job: HANDLE, process: HANDLE) -> bool {
        unsafe {
            match AssignProcessToJobObject(job, process) {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("[win32] AssignProcessToJobObject failed: {error}");
                    false
                }
            }
        }
    }

    /// Base64-encoded UTF-16LE for PowerShell `-EncodedCommand`.
    pub fn encode_powershell_command(command: &str) -> String {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let data: Vec<u8> = command
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        STANDARD.encode(&data)
    }
}

pub struct JobGuard {
    handle: windows::Win32::Foundation::HANDLE,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        log::debug!("[job] closing job object handle");
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

unsafe impl Send for JobGuard {}
unsafe impl Sync for JobGuard {}

fn create_child_job_guard(child: &Child) -> Option<JobGuard> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;

    let job = win32::create_kill_on_close_job()?;
    let process_handle = HANDLE(child.as_raw_handle());
    if win32::assign_process_to_job(job, process_handle) {
        log::info!(
            "[job] child pid={} assigned to kill-on-close job object",
            child.id(),
        );
        Some(JobGuard { handle: job })
    } else {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(job);
        }
        None
    }
}

pub fn terminate_process(process_id: u32) -> bool {
    log::info!("[process] terminating pid {} via native API", process_id);
    win32::terminate(process_id, 0)
}

pub fn elevate_terminate_process(process_id: u32) -> bool {
    log::info!(
        "[process] terminating pid {} via native API (elevated)",
        process_id
    );
    win32::terminate(process_id, 0)
}

pub fn spawn_client(
    binary: &str,
    configuration_path: &std::path::Path,
    needs_elevation: bool,
) -> io::Result<ChildProcess> {
    use std::os::windows::process::CommandExt;

    if needs_elevation && !is_running_as_admin() {
        log::info!(
            "[connect] elevation required, spawning elevated: {} -c {}",
            binary,
            configuration_path.display(),
        );
        // ShellExecuteEx "runas" triggers UAC. Output goes to a temp log file;
        // a separate exit-marker file signals completion.
        let log_path = elevated_log_path();
        let exit_marker = elevated_exit_marker_path();
        let _ = std::fs::write(&log_path, "");
        let _ = std::fs::remove_file(&exit_marker);

        // Encode as Base64 UTF-16LE to avoid quoting issues.
        let ps_command = format!(
            "& '{}' -c '{}' 2>&1 | Out-File -FilePath '{}' -Encoding utf8; \
             $LASTEXITCODE | Out-File -FilePath '{}'",
            binary.replace('\'', "''"),
            configuration_path.display().to_string().replace('\'', "''"),
            log_path.display().to_string().replace('\'', "''"),
            exit_marker.display().to_string().replace('\'', "''"),
        );
        let encoded = win32::encode_powershell_command(&ps_command);
        let ps_args = format!(
            "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -EncodedCommand {encoded}",
        );

        if !win32::shell_execute_runas_with_args("powershell.exe", Some(&ps_args)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "UAC elevation was denied or ShellExecuteEx failed. \
                 You can also run TrustTunnel UI as Administrator.",
            ));
        }

        Ok(ChildProcess::Elevated {
            log_path,
            exit_marker_path: exit_marker,
        })
    } else {
        log::info!(
            "[connect] spawning: {} -c {}",
            binary,
            configuration_path.display(),
        );
        Command::new(binary)
            .arg("-c")
            .arg(configuration_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .map(|child| {
                let _job_guard = create_child_job_guard(&child);
                ChildProcess::Direct { child, _job_guard }
            })
    }
}

const INTERNET_SETTINGS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

fn notify_proxy_settings_changed() {
    use windows::Win32::Networking::WinInet::{
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
    };

    unsafe {
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
}

pub fn set_system_proxy(host: &str, port: u16) -> String {
    let proxy_value = format!("socks={}:{}", host, port);

    log::info!("[proxy] setting system SOCKS5 proxy to {}:{}", host, port);

    match windows_registry::CURRENT_USER.create(INTERNET_SETTINGS_KEY) {
        Ok(key) => {
            if let Err(error) = key.set_u32("ProxyEnable", 1) {
                log::warn!("[proxy] failed to set ProxyEnable: {error}");
            }
            if let Err(error) = key.set_string("ProxyServer", &proxy_value) {
                log::warn!("[proxy] failed to set ProxyServer: {error}");
            }
            if let Err(error) = key.set_string(
                "ProxyOverride",
                "localhost;127.*;10.*;172.16.*;192.168.*;<local>",
            ) {
                log::warn!("[proxy] failed to set ProxyOverride: {error}");
            }
        }
        Err(error) => {
            log::warn!("[proxy] failed to open registry key: {error}");
        }
    }

    notify_proxy_settings_changed();

    if let Ok(key) = windows_registry::CURRENT_USER.open(INTERNET_SETTINGS_KEY) {
        let verify_enable = key.get_u32("ProxyEnable").unwrap_or(0);
        let verify_server = key.get_string("ProxyServer").unwrap_or_default();
        let verify_override = key.get_string("ProxyOverride").unwrap_or_default();
        log::info!(
            "[proxy] registry verify: ProxyEnable={}, ProxyServer={}, ProxyOverride={}",
            verify_enable,
            verify_server,
            verify_override,
        );
    }

    let detail = format!(
        "System proxy configured via registry (SOCKS5 {}:{})",
        host, port
    );
    log::info!("[proxy] {detail}");
    detail
}

pub fn clear_system_proxy() {
    log::info!("[proxy] clearing system proxy settings");

    match windows_registry::CURRENT_USER.create(INTERNET_SETTINGS_KEY) {
        Ok(key) => {
            if let Err(error) = key.set_u32("ProxyEnable", 0) {
                log::warn!("[proxy] failed to set ProxyEnable: {error}");
            }
            let _ = key.remove_value("ProxyServer");
            let _ = key.remove_value("ProxyOverride");
        }
        Err(error) => {
            log::warn!("[proxy] failed to open registry key: {error}");
        }
    }

    notify_proxy_settings_changed();

    if let Ok(key) = windows_registry::CURRENT_USER.open(INTERNET_SETTINGS_KEY) {
        let verify_enable = key.get_u32("ProxyEnable").unwrap_or(u32::MAX);
        let verify_server = key
            .get_string("ProxyServer")
            .unwrap_or_else(|_| "<removed>".into());
        log::info!(
            "[proxy] cleared — registry verify: ProxyEnable={}, ProxyServer={}",
            verify_enable,
            verify_server,
        );
    } else {
        log::info!("[proxy] cleared — registry ProxyEnable set to 0");
    }
}

pub struct RegistryProxy;

impl ProxyBackend for RegistryProxy {
    fn name(&self) -> &str {
        "Windows Registry"
    }

    fn set(&mut self, host: &str, port: u16) -> Result<String, String> {
        let detail = set_system_proxy(host, port);
        Ok(detail)
    }

    fn clear(&mut self) {
        clear_system_proxy();
    }
}

pub fn find_client_binary() -> (String, bool) {
    let binary_name = "trusttunnel_client.exe";

    let mut command = Command::new("where");
    command
        .arg(binary_name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    if let Ok(output) = command.output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !path.is_empty() {
            log::info!("[binary] found via where: {path}");
            return (path, true);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(program_files) = std::env::var("ProgramFiles") {
        candidates.push(
            PathBuf::from(&program_files)
                .join("TrustTunnel")
                .join(binary_name),
        );
    }
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(&program_files_x86)
                .join("TrustTunnel")
                .join(binary_name),
        );
    }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(&local_app_data)
                .join("TrustTunnel")
                .join(binary_name),
        );
    }
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(directory) = exe_path.parent()
    {
        candidates.push(directory.join(binary_name));
    }

    for candidate in &candidates {
        if candidate.exists() {
            let path = candidate.to_string_lossy().to_string();
            log::info!("[binary] found on disk: {path}");
            return (path, true);
        }
    }

    log::warn!("[binary] {binary_name} not found in search paths");
    (binary_name.to_string(), false)
}

pub fn check_tun_device() -> bool {
    let mut search_paths: Vec<PathBuf> = Vec::new();

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(directory) = exe_path.parent()
    {
        search_paths.push(directory.join("wintun.dll"));
    }
    if let Ok(system_root) = std::env::var("SystemRoot") {
        search_paths.push(
            PathBuf::from(&system_root)
                .join("System32")
                .join("wintun.dll"),
        );
    }
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        search_paths.push(
            PathBuf::from(&program_files)
                .join("TrustTunnel")
                .join("wintun.dll"),
        );
    }
    if let Ok(current_dir) = std::env::current_dir() {
        search_paths.push(current_dir.join("wintun.dll"));
    }

    for path in &search_paths {
        if path.exists() {
            log::info!("[preflight] wintun.dll found: {}", path.display());
            return true;
        }
    }

    log::warn!(
        "[preflight] wintun.dll not found in any search path: {:?}",
        search_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
    );
    false
}

pub fn check_elevation_available() -> bool {
    if is_running_as_admin() {
        log::debug!("[preflight] running with administrator privileges");
    } else {
        log::info!("[preflight] not running as administrator — TUN mode will use UAC elevation");
    }
    true
}

pub fn is_running_as_admin() -> bool {
    use windows::Win32::UI::Shell::IsUserAnAdmin;
    unsafe { IsUserAnAdmin().as_bool() }
}

pub fn elevated_log_path() -> PathBuf {
    let pid = std::process::id();
    std::env::temp_dir().join(format!("trusttunnel_elevated_{pid}.log"))
}

pub fn elevated_exit_marker_path() -> PathBuf {
    let pid = std::process::id();
    std::env::temp_dir().join(format!("trusttunnel_elevated_{pid}.exit"))
}

/// Kills by image name — we don't have a handle to the UAC-elevated process.
pub fn terminate_elevated_client() -> bool {
    let binary_name = "trusttunnel_client.exe";
    log::info!(
        "[process] terminating elevated client by image name: {}",
        binary_name
    );
    let killed = run_silent("taskkill", &["/F", "/IM", binary_name]);
    let _ = std::fs::write(elevated_exit_marker_path(), "terminated");
    killed
}

pub fn cleanup_elevated_files() {
    let _ = std::fs::remove_file(elevated_log_path());
    let _ = std::fs::remove_file(elevated_exit_marker_path());
}

pub fn cleanup_stale_elevated_files() {
    let temp = std::env::temp_dir();
    let entries = match std::fs::read_dir(&temp) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with("trusttunnel_elevated_") && name.ends_with(".log"))
            || (name.starts_with("trusttunnel_elevated_") && name.ends_with(".exit"))
        {
            let own_log = elevated_log_path();
            let own_exit = elevated_exit_marker_path();
            let path = entry.path();
            if path == own_log || path == own_exit {
                continue;
            }
            log::info!("[startup] removing stale elevated file: {}", path.display());
            let _ = std::fs::remove_file(&path);
        }
    }
}

pub fn install_ctrl_handler() {
    use windows::{
        Win32::System::Console::{
            CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler,
        },
        core::BOOL,
    };

    unsafe extern "system" fn handler(ctrl_type: u32) -> BOOL {
        match ctrl_type {
            x if x == CTRL_CLOSE_EVENT || x == CTRL_LOGOFF_EVENT || x == CTRL_SHUTDOWN_EVENT => {
                log::info!(
                    "[ctrl_handler] received control event {x}, performing emergency cleanup",
                );
                super::proxy::emergency_clear();
                super::dns::emergency_clear();
                terminate_elevated_client();
                cleanup_elevated_files();
                BOOL(1)
            }
            _ => BOOL(0),
        }
    }

    if let Err(error) = unsafe { SetConsoleCtrlHandler(Some(handler), true) } {
        log::warn!("[startup] SetConsoleCtrlHandler failed: {error}");
    } else {
        log::info!("[startup] console control handler installed");
    }
}

pub fn cleanup_stale_system_proxy() {
    let Ok(key) = windows_registry::CURRENT_USER.open(INTERNET_SETTINGS_KEY) else {
        return;
    };
    let enabled: u32 = key.get_u32("ProxyEnable").unwrap_or(0);
    if enabled == 0 {
        return;
    }
    let server: String = key.get_string("ProxyServer").unwrap_or_default();
    let expected = format!("socks={}", crate::configuration::PROXY_LISTEN_ADDRESS);
    if server == expected {
        let client_running = {
            use std::os::windows::process::CommandExt;
            Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq trusttunnel_client.exe", "/NH"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map(|output| {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout.contains("trusttunnel_client.exe")
                })
                .unwrap_or(false)
        };
        if !client_running {
            log::warn!(
                "[startup] stale system proxy detected ({}), clearing",
                server,
            );
            clear_system_proxy();
        }
    }
}
