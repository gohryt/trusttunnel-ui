use super::{
    dns::{self, DnsBackend},
    run_silent,
};

const CLEAR_SCRIPT: &str = r#"Get-NetAdapter | Where-Object {$_.Status -eq 'Up' -and $_.InterfaceDescription -notlike '*TUN*' -and $_.InterfaceDescription -notlike '*TAP*' -and $_.InterfaceDescription -notlike '*Loopback*'} | ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ResetServerAddresses }"#;

const POWERSHELL_ARGS: &[&str] = &[
    "-NoProfile",
    "-NonInteractive",
    "-WindowStyle",
    "Hidden",
    "-Command",
];

pub fn is_available() -> bool {
    let available = super::is_running_as_admin();
    if available {
        log::debug!("[preflight] PowerShell DNS override is available (running as admin)");
    } else {
        log::info!(
            "[preflight] PowerShell DNS override unavailable — not running as administrator"
        );
    }
    available
}

pub struct PowerShellDns;

impl PowerShellDns {
    pub fn new() -> Self {
        Self
    }
}

impl DnsBackend for PowerShellDns {
    fn name(&self) -> &str {
        "PowerShell"
    }

    fn set(&mut self, upstreams: &[&str]) -> Result<String, String> {
        log::info!("[dns] setting DNS via PowerShell");

        let servers: Vec<&str> = if upstreams.is_empty() {
            dns::DEFAULT_DNS_SERVERS.to_vec()
        } else {
            upstreams.to_vec()
        };
        let servers_str = servers.join(",");
        let servers_arg = format!("'{}'", servers_str.replace(',', "','"));
        let set_script = format!(
            r#"Get-NetAdapter | Where-Object {{$_.Status -eq 'Up' -and $_.InterfaceDescription -notlike '*TUN*' -and $_.InterfaceDescription -notlike '*TAP*' -and $_.InterfaceDescription -notlike '*Loopback*'}} | ForEach-Object {{ Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ServerAddresses {} }}"#,
            servers_arg
        );

        let mut args = POWERSHELL_ARGS.to_vec();
        args.push(&set_script);

        if run_silent("powershell", &args) {
            let detail = format!("DNS configured via PowerShell ({servers_str})");
            log::info!("[dns] {detail}");
            Ok(detail)
        } else {
            let detail = "Failed to set DNS via PowerShell (may require administrator privileges)"
                .to_string();
            log::warn!("[dns] {detail}");
            Err(detail)
        }
    }

    fn clear(&mut self) {
        log::info!("[dns] restoring DNS via PowerShell");

        let mut args = POWERSHELL_ARGS.to_vec();
        args.push(CLEAR_SCRIPT);

        if !run_silent("powershell", &args) {
            log::warn!("[dns] failed to restore DNS settings");
        }
    }
}
