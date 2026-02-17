use super::{
    dns::{self, DnsBackend},
    run_silent, run_silent_with_output,
};

pub fn is_available() -> bool {
    let active = std::path::Path::new("/run/systemd/resolve/stub-resolv.conf").exists()
        || run_silent("systemctl", &["is-active", "--quiet", "systemd-resolved"]);
    if active {
        log::debug!("[preflight] systemd-resolved is active");
    } else {
        log::info!("[preflight] systemd-resolved is not active");
    }
    active
}

pub struct ResolvedDns {
    interface: Option<String>,
}

impl ResolvedDns {
    pub fn new() -> Self {
        Self { interface: None }
    }
}

impl DnsBackend for ResolvedDns {
    fn name(&self) -> &str {
        "systemd-resolved"
    }

    fn set(&mut self, upstreams: &[&str]) -> Result<String, String> {
        log::info!("[dns] setting DNS via systemd-resolved");

        let interface = match find_tun_interface() {
            Some(name) => name,
            None => {
                let detail = "DNS via systemd-resolved: no TUN interface found".to_string();
                log::warn!("[dns] {detail}");
                return Err(detail);
            }
        };

        let mut dns_args: Vec<&str> = vec!["dns", &interface];
        let servers_slice = if upstreams.is_empty() {
            dns::DEFAULT_DNS_SERVERS
        } else {
            upstreams
        };
        dns_args.extend(servers_slice);

        if !resolvectl(&dns_args) {
            let detail = format!("Failed to set DNS servers on {interface} via resolvectl");
            log::warn!("[dns] {detail}");
            return Err(detail);
        }

        if !resolvectl(&["domain", &interface, "~."]) {
            log::warn!(
                "[dns] failed to set routing domain on {interface}, \
                 DNS may not route through tunnel"
            );
        }

        if !resolvectl(&["default-route", &interface, "true"]) {
            log::warn!("[dns] failed to set default-route on {interface}");
        }

        let (_, status) = run_silent_with_output("resolvectl", &["status", &interface]);
        if !status.is_empty() {
            for line in status.lines().take(8) {
                log::debug!("[dns] resolvectl status: {}", line.trim());
            }
        }

        self.interface = Some(interface.clone());

        let servers_str = servers_slice.join(", ");
        let detail = format!("DNS configured via systemd-resolved on {interface} ({servers_str})");
        log::info!("[dns] {detail}");
        Ok(detail)
    }

    fn clear(&mut self) {
        let interface = self.interface.take().or_else(find_tun_interface);

        let interface = match interface {
            Some(name) => name,
            None => {
                log::info!(
                    "[dns] no stored/detected TUN interface to revert — \
                     systemd-resolved likely cleaned up automatically"
                );
                return;
            }
        };

        log::info!("[dns] reverting DNS on {interface} via systemd-resolved");

        if resolvectl(&["revert", &interface]) {
            log::info!("[dns] successfully reverted DNS on {interface}");
        } else {
            log::info!(
                "[dns] resolvectl revert {interface} failed \
                 (interface may already be destroyed)"
            );
        }
    }
}

fn find_tun_interface() -> Option<String> {
    let (success, output) = run_silent_with_output("ip", &["-o", "link", "show", "type", "tun"]);
    if success
        && !output.trim().is_empty()
        && let Some(name) = output.lines().next().and_then(|line| {
            line.split_whitespace()
                .nth(1)
                .map(|field| field.trim_end_matches(':').to_string())
        })
    {
        log::debug!("[resolved] found TUN interface via ip: {name}");
        return Some(name);
    }

    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let tun_flags_path = entry.path().join("tun_flags");
            if tun_flags_path.exists() {
                log::debug!("[resolved] found TUN interface via sysfs: {name_str}");
                return Some(name_str.into_owned());
            }
        }
    }

    for candidate in &["tun0", "tun1", "tun2", "tun3"] {
        let path = format!("/sys/class/net/{candidate}");
        if std::path::Path::new(&path).exists() {
            log::debug!("[resolved] found TUN interface by name probe: {candidate}");
            return Some(candidate.to_string());
        }
    }

    log::warn!("[resolved] no TUN interface found");
    None
}

fn resolvectl(args: &[&str]) -> bool {
    if run_silent("resolvectl", args) {
        return true;
    }

    log::debug!(
        "[resolved] resolvectl {} failed, retrying with pkexec",
        args.first().unwrap_or(&""),
    );

    let mut pkexec_args = vec!["resolvectl"];
    pkexec_args.extend_from_slice(args);
    run_silent("pkexec", &pkexec_args)
}
