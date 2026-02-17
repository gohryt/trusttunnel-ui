#[cfg(target_os = "linux")]
use super::{resolvconf, resolved, run_silent, run_silent_with_output};

#[cfg(target_os = "windows")]
use super::{powershell_dns, run_silent};

pub const DEFAULT_DNS_SERVERS: &[&str] = &["1.1.1.1", "1.0.0.1"];

pub trait DnsBackend: Send {
    fn name(&self) -> &str;

    /// Empty `upstreams` means use defaults.
    fn set(&mut self, upstreams: &[&str]) -> Result<String, String>;

    fn clear(&mut self);
}

#[cfg(target_os = "linux")]
pub fn detect() -> Option<Box<dyn DnsBackend>> {
    if resolved::is_available() {
        log::info!("[dns] selected backend: systemd-resolved");
        return Some(Box::new(resolved::ResolvedDns::new()));
    }

    if resolvconf::is_available() {
        log::info!("[dns] selected backend: resolvconf");
        return Some(Box::new(resolvconf::ResolvconfDns::new()));
    }

    log::info!("[dns] no DNS backend available");
    None
}

#[cfg(target_os = "windows")]
pub fn detect() -> Option<Box<dyn DnsBackend>> {
    if powershell_dns::is_available() {
        log::info!("[dns] selected backend: PowerShell");
        return Some(Box::new(powershell_dns::PowerShellDns::new()));
    }

    log::info!("[dns] no DNS backend available (not running as administrator)");
    None
}

#[cfg(target_os = "linux")]
pub fn emergency_clear() {
    log::error!("[dns] emergency cleanup — attempting all known backends");

    let (ok, output) = run_silent_with_output("ip", &["-o", "link", "show", "type", "tun"]);
    if ok {
        for line in output.lines() {
            if let Some(name) = line
                .split_whitespace()
                .nth(1)
                .map(|field| field.trim_end_matches(':'))
            {
                log::error!("[dns] emergency: resolvectl revert {name}");
                if !run_silent("resolvectl", &["revert", name]) {
                    log::error!("[dns] emergency: retrying with pkexec resolvectl revert {name}");
                    let _ = run_silent("pkexec", &["resolvectl", "revert", name]);
                }
            }
        }
    }

    log::error!("[dns] emergency: resolvconf -d tun-trusttunnel");
    if !run_silent("resolvconf", &["-d", "tun-trusttunnel"]) {
        log::error!("[dns] emergency: retrying with pkexec resolvconf -d tun-trusttunnel");
        let _ = run_silent("pkexec", &["resolvconf", "-d", "tun-trusttunnel"]);
    }
}

#[cfg(target_os = "windows")]
pub fn emergency_clear() {
    log::error!("[dns] emergency cleanup — restoring DNS via PowerShell");

    let script = r#"Get-NetAdapter | Where-Object {$_.Status -eq 'Up' -and $_.InterfaceDescription -notlike '*TUN*' -and $_.InterfaceDescription -notlike '*TAP*' -and $_.InterfaceDescription -notlike '*Loopback*'} | ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ResetServerAddresses }"#;

    let _ = run_silent(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ],
    );
}
