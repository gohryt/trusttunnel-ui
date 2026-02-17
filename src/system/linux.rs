use std::{
    io,
    path::Path,
    process::{Command, Stdio},
};

use super::{ChildProcess, run_silent, run_silent_with_output};

pub fn terminate_process(process_id: u32) -> bool {
    let process_id_string = process_id.to_string();
    run_silent("kill", &["-INT", &process_id_string])
}

pub fn elevate_terminate_process(process_id: u32) -> bool {
    let process_id_string = process_id.to_string();
    run_silent("pkexec", &["kill", "-INT", &process_id_string])
}

pub fn spawn_client(
    binary: &str,
    configuration_path: &Path,
    needs_elevation: bool,
) -> io::Result<ChildProcess> {
    if needs_elevation {
        log::info!(
            "[connect] spawning: pkexec {} -c {}",
            binary,
            configuration_path.display(),
        );
        Command::new("pkexec")
            .arg(binary)
            .arg("-c")
            .arg(configuration_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map(|child| ChildProcess::Direct { child })
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
            .spawn()
            .map(|child| ChildProcess::Direct { child })
    }
}

pub fn find_client_binary() -> (String, bool) {
    let candidates = [
        "trusttunnel_client",
        "/opt/trusttunnel_client/trusttunnel_client",
        "/usr/local/bin/trusttunnel_client",
        "/usr/bin/trusttunnel_client",
    ];

    for candidate in &candidates {
        if let Ok(output) = Command::new("which").arg(candidate).output()
            && output.status.success()
        {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            log::info!("[binary] found via which: {candidate} → {path}");
            return (path, true);
        }
        if Path::new(candidate).exists() {
            log::info!("[binary] found on disk: {candidate}");
            return (candidate.to_string(), true);
        }
    }

    log::warn!("[binary] trusttunnel_client not found in search paths");
    ("trusttunnel_client".to_string(), false)
}

pub fn check_tun_device() -> bool {
    let path = Path::new("/dev/net/tun");

    if path.exists() {
        log::debug!("[preflight] /dev/net/tun exists");
        true
    } else {
        log::warn!("[preflight] /dev/net/tun not found — the tun kernel module may not be loaded");
        false
    }
}

pub fn check_elevation_available() -> bool {
    let (success, _) = run_silent_with_output("which", &["pkexec"]);

    if success {
        log::debug!("[preflight] pkexec is available");
    } else {
        log::warn!("[preflight] pkexec not found — TUN mode will not work without root privileges");
    }

    success
}
