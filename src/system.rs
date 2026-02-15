use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

pub fn run_silent_with_output(program: &str, arguments: &[&str]) -> (bool, String) {
    log::debug!("[cmd] {} {}", program, arguments.join(" "));
    match Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
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
            (success, stderr)
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

fn is_kde() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    desktop == "KDE" || desktop == "Trinity"
}

fn kwriteconfig_command() -> &'static str {
    match std::env::var("KDE_SESSION_VERSION")
        .unwrap_or_default()
        .as_str()
    {
        "6" => "kwriteconfig6",
        "5" => "kwriteconfig5",
        _ => "kwriteconfig6",
    }
}

fn kioslaverc_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kioslaverc")
}

pub fn set_system_proxy(host: &str, port: u16) -> String {
    let port_str = port.to_string();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let kde_desktop = is_kde();

    log::info!(
        "[proxy] setting system SOCKS5 proxy to {}:{} (desktop={})",
        host,
        port,
        desktop,
    );

    run_silent(
        "gsettings",
        &["set", "org.gnome.system.proxy.socks", "host", host],
    );
    run_silent(
        "gsettings",
        &["set", "org.gnome.system.proxy.socks", "port", &port_str],
    );

    for protocol in &["http", "https", "ftp"] {
        run_silent(
            "gsettings",
            &[
                "set",
                &format!("org.gnome.system.proxy.{}", protocol),
                "host",
                "",
            ],
        );
        run_silent(
            "gsettings",
            &[
                "set",
                &format!("org.gnome.system.proxy.{}", protocol),
                "port",
                "0",
            ],
        );
    }

    run_silent(
        "gsettings",
        &["set", "org.gnome.system.proxy", "use-same-proxy", "false"],
    );
    run_silent(
        "gsettings",
        &["set", "org.gnome.system.proxy", "mode", "manual"],
    );

    let (_, verify_mode) =
        run_silent_with_output("gsettings", &["get", "org.gnome.system.proxy", "mode"]);
    let (_, verify_host) = run_silent_with_output(
        "gsettings",
        &["get", "org.gnome.system.proxy.socks", "host"],
    );
    let (_, verify_port) = run_silent_with_output(
        "gsettings",
        &["get", "org.gnome.system.proxy.socks", "port"],
    );
    let (_, verify_same) = run_silent_with_output(
        "gsettings",
        &["get", "org.gnome.system.proxy", "use-same-proxy"],
    );
    log::info!(
        "[proxy] gsettings verify: mode={}, socks_host={}, socks_port={}, use-same-proxy={}",
        verify_mode.trim(),
        verify_host.trim(),
        verify_port.trim(),
        verify_same.trim(),
    );

    if kde_desktop {
        let kwriteconfig = kwriteconfig_command();
        let kioslaverc_path = kioslaverc_config_path();
        let kioslaverc_path_string = kioslaverc_path.to_string_lossy().to_string();
        let proxy_url = format!("socks5://{}:{}", host, port);

        log::debug!(
            "[proxy] KDE: kwriteconfig={kwriteconfig}, kioslaverc={kioslaverc_path_string}"
        );

        run_silent(
            kwriteconfig,
            &[
                "--file",
                &kioslaverc_path_string,
                "--group",
                "Proxy Settings",
                "--key",
                "ProxyType",
                "1",
            ],
        );
        run_silent(
            kwriteconfig,
            &[
                "--file",
                &kioslaverc_path_string,
                "--group",
                "Proxy Settings",
                "--key",
                "socksProxy",
                &proxy_url,
            ],
        );
        for protocol in &["httpProxy", "httpsProxy", "ftpProxy"] {
            run_silent(
                kwriteconfig,
                &[
                    "--file",
                    &kioslaverc_path_string,
                    "--group",
                    "Proxy Settings",
                    "--key",
                    protocol,
                    "",
                ],
            );
        }
        run_silent(
            kwriteconfig,
            &[
                "--file",
                &kioslaverc_path_string,
                "--group",
                "Proxy Settings",
                "--key",
                "NoProxyFor",
                "localhost,127.0.0.0/8,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16",
            ],
        );
        run_silent(
            "dbus-send",
            &[
                "--type=signal",
                "/KIO/Scheduler",
                "org.kde.KIO.Scheduler.reparseSlaveConfiguration",
                "string:''",
            ],
        );
    }

    let targets = if kde_desktop {
        "gsettings + KDE KIO"
    } else {
        "gsettings"
    };
    let desktop_name = if desktop.is_empty() {
        "unknown"
    } else {
        desktop.as_str()
    };
    let detail = format!(
        "System proxy configured via {} (desktop: {})",
        targets, desktop_name,
    );
    log::info!("[proxy] {detail}");
    detail
}

pub fn clear_system_proxy() {
    log::info!("[proxy] clearing system proxy settings");

    run_silent(
        "gsettings",
        &["set", "org.gnome.system.proxy", "mode", "none"],
    );
    run_silent(
        "gsettings",
        &["set", "org.gnome.system.proxy", "use-same-proxy", "true"],
    );

    if is_kde() {
        let kwriteconfig = kwriteconfig_command();
        let kioslaverc_path = kioslaverc_config_path();
        let kioslaverc_path_string = kioslaverc_path.to_string_lossy().to_string();

        run_silent(
            kwriteconfig,
            &[
                "--file",
                &kioslaverc_path_string,
                "--group",
                "Proxy Settings",
                "--key",
                "ProxyType",
                "0",
            ],
        );
        run_silent(
            "dbus-send",
            &[
                "--type=signal",
                "/KIO/Scheduler",
                "org.kde.KIO.Scheduler.reparseSlaveConfiguration",
                "string:''",
            ],
        );
    }

    let (_, verify_mode) =
        run_silent_with_output("gsettings", &["get", "org.gnome.system.proxy", "mode"]);
    log::info!("[proxy] cleared — gsettings mode={}", verify_mode.trim());
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
        if std::path::Path::new(candidate).exists() {
            log::info!("[binary] found on disk: {candidate}");
            return (candidate.to_string(), true);
        }
    }

    log::warn!("[binary] trusttunnel_client not found in search paths");
    ("trusttunnel_client".to_string(), false)
}

pub fn check_tun_device() -> bool {
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/net/tun")
    {
        Ok(_) => {
            log::debug!("[preflight] /dev/net/tun is available");
            true
        }
        Err(error) => {
            log::warn!("[preflight] /dev/net/tun not available: {error}");
            false
        }
    }
}

pub fn check_systemd_resolved() -> bool {
    let active = std::path::Path::new("/run/systemd/resolve/stub-resolv.conf").exists()
        || run_silent("systemctl", &["is-active", "--quiet", "systemd-resolved"]);
    if active {
        log::debug!("[preflight] systemd-resolved is active");
    } else {
        log::info!("[preflight] systemd-resolved is not active, disabling change_system_dns");
    }
    active
}

pub fn check_resolvconf() -> bool {
    let (success, _) = run_silent_with_output("which", &["resolvconf"]);
    if success {
        log::debug!("[preflight] resolvconf is available");
    } else {
        log::info!("[preflight] resolvconf not found");
    }
    success
}

const RESOLVCONF_INTERFACE: &str = "tun-trusttunnel";

pub fn set_dns_resolvconf() -> String {
    log::info!("[dns] setting DNS via resolvconf");

    let result = Command::new("resolvconf")
        .args(["-a", RESOLVCONF_INTERFACE, "-m", "0", "-x"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(ref mut stdin) = child.stdin {
                use std::io::Write;
                if let Err(error) = writeln!(stdin, "nameserver 1.1.1.1") {
                    log::warn!("[dns] failed to write resolvconf entry: {error}");
                }
                if let Err(error) = writeln!(stdin, "nameserver 1.0.0.1") {
                    log::warn!("[dns] failed to write resolvconf entry: {error}");
                }
            }
            child.wait()
        });

    match result {
        Ok(status) if status.success() => {
            let detail = "DNS configured via resolvconf (1.1.1.1, 1.0.0.1)".to_string();
            log::info!("[dns] {detail}");
            detail
        }
        Ok(status) => {
            let detail = format!("resolvconf exited with {status}");
            log::warn!("[dns] {detail}");
            detail
        }
        Err(error) => {
            let detail = format!("Failed to run resolvconf: {error}");
            log::warn!("[dns] {detail}");
            detail
        }
    }
}

pub fn clear_dns_resolvconf() {
    log::info!("[dns] clearing DNS via resolvconf");
    if !run_silent("resolvconf", &["-d", RESOLVCONF_INTERFACE]) {
        log::warn!("[dns] resolvconf -d failed");
    }
}

pub fn check_pkexec() -> bool {
    let (success, _) = run_silent_with_output("which", &["pkexec"]);
    if success {
        log::debug!("[preflight] pkexec is available");
    } else {
        log::warn!("[preflight] pkexec not found — TUN mode will not work without root privileges");
    }
    success
}

pub fn check_binary_works(binary: &str, needs_root: bool) -> Option<String> {
    if needs_root {
        log::debug!("[preflight] skipping elevated binary check (would prompt for auth)");
        return None;
    }

    log::debug!("[preflight] testing binary: {binary} --help");

    match Command::new(binary)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
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

pub fn open_file_dialog(title: &str, filter_label: &str, filter_glob: &str) -> Option<PathBuf> {
    let zenity_filter = format!("{} ({})|{}", filter_label, filter_glob, filter_glob);
    let result = Command::new("zenity")
        .args([
            "--file-selection",
            &format!("--title={title}"),
            &format!("--file-filter={zenity_filter}"),
        ])
        .output()
        .or_else(|_| {
            Command::new("kdialog")
                .args(["--getopenfilename", ".", filter_glob])
                .output()
        });

    match result {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            }
        }
        _ => {
            log::warn!("[dialog] no file dialog available (install zenity or kdialog)");
            None
        }
    }
}

pub fn parse_host_port(address: &str) -> (String, u16) {
    if let Some(index) = address.rfind(':') {
        let host = &address[..index];
        let port = address[index + 1..].parse::<u16>().unwrap_or(1080);
        (host.to_string(), port)
    } else {
        (address.to_string(), 1080)
    }
}
