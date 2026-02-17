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

        let dns_servers: Vec<&str> = if upstreams.is_empty() {
            dns::DEFAULT_DNS_SERVERS.to_vec()
        } else {
            upstreams.to_vec()
        };
        let dns_servers_string = dns_servers.join(",");
        let dns_servers_argument = format!("'{}'", dns_servers_string.replace(',', "','"));
        let set_script = format!(
            r#"Get-NetAdapter | Where-Object {{$_.Status -eq 'Up' -and $_.InterfaceDescription -notlike '*TUN*' -and $_.InterfaceDescription -notlike '*TAP*' -and $_.InterfaceDescription -notlike '*Loopback*'}} | ForEach-Object {{ Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ServerAddresses {} }}"#,
            dns_servers_argument
        );

        let mut argument_list = POWERSHELL_ARGS.to_vec();
        argument_list.push(&set_script);

        if run_silent("powershell", &argument_list) {
            let detail = format!("DNS configured via PowerShell ({dns_servers_string})");
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

        let mut argument_list = POWERSHELL_ARGS.to_vec();
        argument_list.push(CLEAR_SCRIPT);

        if !run_silent("powershell", &argument_list) {
            log::warn!("[dns] failed to restore DNS settings");
        }
    }
}
