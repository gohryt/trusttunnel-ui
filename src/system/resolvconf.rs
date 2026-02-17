use std::process::{Command, Stdio};

use super::{
    dns::{self, DnsBackend},
    run_silent, run_silent_with_output,
};

const RESOLVCONF_INTERFACE: &str = "tun-trusttunnel";

pub fn is_available() -> bool {
    let (success, _) = run_silent_with_output("which", &["resolvconf"]);
    if success {
        log::debug!("[preflight] resolvconf is available");
    } else {
        log::info!("[preflight] resolvconf not found");
    }
    success
}

pub struct ResolvconfDns;

impl ResolvconfDns {
    pub fn new() -> Self {
        Self
    }
}

impl DnsBackend for ResolvconfDns {
    fn name(&self) -> &str {
        "resolvconf"
    }

    fn set(&mut self, upstreams: &[&str]) -> Result<String, String> {
        log::info!("[dns] setting DNS via resolvconf");

        let dns_servers: Vec<&str> = if upstreams.is_empty() {
            dns::DEFAULT_DNS_SERVERS.to_vec()
        } else {
            upstreams.to_vec()
        };
        let dns_servers_string = dns_servers.join(", ");

        let stdin_content = dns_servers
            .iter()
            .map(|server| format!("nameserver {server}"))
            .collect::<Vec<_>>()
            .join("\n");

        if try_resolvconf_set(&stdin_content) {
            let detail = format!("DNS configured via resolvconf ({dns_servers_string})");
            log::info!("[dns] {detail}");
            return Ok(detail);
        }

        log::debug!("[resolvconf] direct resolvconf -a failed, retrying with pkexec",);

        if try_resolvconf_set_elevated(&stdin_content) {
            let detail =
                format!("DNS configured via resolvconf with pkexec ({dns_servers_string})");
            log::info!("[dns] {detail}");
            return Ok(detail);
        }

        let detail = "Failed to set DNS via resolvconf (tried both direct and pkexec)".to_string();
        log::warn!("[dns] {detail}");
        Err(detail)
    }

    fn clear(&mut self) {
        log::info!("[dns] clearing DNS via resolvconf");
        if resolvconf_delete() {
            return;
        }
        log::warn!("[dns] resolvconf -d failed");
    }
}

fn try_resolvconf_set(stdin_content: &str) -> bool {
    let result = Command::new("resolvconf")
        .args(["-a", RESOLVCONF_INTERFACE, "-m", "0", "-x"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                if let Err(error) = write!(stdin, "{stdin_content}") {
                    log::warn!("[dns] failed to write resolvconf entry: {error}");
                }
            }
            child.wait()
        });

    match result {
        Ok(status) => status.success(),
        Err(error) => {
            log::debug!("[dns] resolvconf spawn error: {error}");
            false
        }
    }
}

/// pkexec does not pass stdin through, so we use `sh -c` with a printf pipeline.
fn try_resolvconf_set_elevated(stdin_content: &str) -> bool {
    let escaped = stdin_content.replace('\'', "'\\''");
    let shell_command =
        format!("printf '%s\\n' '{escaped}' | resolvconf -a {RESOLVCONF_INTERFACE} -m 0 -x",);

    run_silent("pkexec", &["sh", "-c", &shell_command])
}

fn resolvconf_delete() -> bool {
    if run_silent("resolvconf", &["-d", RESOLVCONF_INTERFACE]) {
        return true;
    }

    log::debug!("[resolvconf] resolvconf -d failed, retrying with pkexec",);

    run_silent("pkexec", &["resolvconf", "-d", RESOLVCONF_INTERFACE])
}
