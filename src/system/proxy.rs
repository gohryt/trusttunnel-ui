#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use super::{run_silent, run_silent_with_output};

pub trait ProxyBackend: Send {
    fn name(&self) -> &str;
    fn set(&mut self, host: &str, port: u16) -> Result<String, String>;
    fn clear(&mut self);
}

/// Multiple backends may be returned (e.g. both GSettings and KDE KIO).
#[cfg(target_os = "linux")]
pub fn detect() -> Vec<Box<dyn ProxyBackend>> {
    let mut backends: Vec<Box<dyn ProxyBackend>> = Vec::new();

    if GnomeProxy::is_available() {
        log::info!("[proxy] detected backend: GSettings (GNOME-based)");
        backends.push(Box::new(GnomeProxy));
    }

    if KdeProxy::is_available() {
        log::info!("[proxy] detected backend: KDE KIO");
        backends.push(Box::new(KdeProxy));
    }

    if backends.is_empty() {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        log::warn!(
            "[proxy] no proxy backend detected for desktop '{}'",
            if desktop.is_empty() {
                "unknown"
            } else {
                &desktop
            },
        );
    }

    backends
}

#[cfg(target_os = "windows")]
pub fn detect() -> Vec<Box<dyn ProxyBackend>> {
    vec![Box::new(super::windows::RegistryProxy)]
}

pub fn set_all(host: &str, port: u16) -> (Vec<Box<dyn ProxyBackend>>, String) {
    let mut backends = detect();
    let mut details: Vec<String> = Vec::new();

    for backend in &mut backends {
        match backend.set(host, port) {
            Ok(detail) => {
                log::info!("[proxy] {} set OK: {detail}", backend.name());
                details.push(detail);
            }
            Err(detail) => {
                log::warn!("[proxy] {} set FAILED: {detail}", backend.name());
                details.push(detail);
            }
        }
    }

    let combined = if details.is_empty() {
        "No proxy backend available".to_string()
    } else {
        details.join("; ")
    };

    (backends, combined)
}

pub fn clear_all(backends: &mut [Box<dyn ProxyBackend>]) {
    for backend in backends {
        log::info!("[proxy] clearing proxy via {}", backend.name());
        backend.clear();
    }
}

/// Does not rely on stored backend state — tries every known mechanism.
#[cfg(target_os = "linux")]
pub fn emergency_clear() {
    log::error!("[proxy] emergency proxy cleanup — trying all known backends");
    GnomeProxy.clear();
    KdeProxy.clear();
}

#[cfg(target_os = "windows")]
pub fn emergency_clear() {
    log::error!("[proxy] emergency proxy cleanup");
    super::windows::RegistryProxy.clear();
}

#[cfg(target_os = "linux")]
pub struct GnomeProxy;

#[cfg(target_os = "linux")]
impl GnomeProxy {
    pub fn is_available() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        desktop.split(':').any(|d| {
            matches!(
                d,
                "GNOME" | "Unity" | "Cinnamon" | "X-Cinnamon" | "MATE" | "Budgie" | "Pantheon"
            )
        })
    }
}

#[cfg(target_os = "linux")]
impl ProxyBackend for GnomeProxy {
    fn name(&self) -> &str {
        "GSettings"
    }

    fn set(&mut self, host: &str, port: u16) -> Result<String, String> {
        let port_string = port.to_string();

        log::info!(
            "[proxy] GSettings: setting SOCKS5 proxy to {}:{}",
            host,
            port,
        );

        run_silent(
            "gsettings",
            &["set", "org.gnome.system.proxy.socks", "host", host],
        );
        run_silent(
            "gsettings",
            &["set", "org.gnome.system.proxy.socks", "port", &port_string],
        );

        for protocol in &["http", "https", "ftp"] {
            run_silent(
                "gsettings",
                &[
                    "set",
                    &format!("org.gnome.system.proxy.{protocol}"),
                    "host",
                    "",
                ],
            );
            run_silent(
                "gsettings",
                &[
                    "set",
                    &format!("org.gnome.system.proxy.{protocol}"),
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
            "[proxy] GSettings verify: mode={}, socks_host={}, socks_port={}, use-same-proxy={}",
            verify_mode.trim(),
            verify_host.trim(),
            verify_port.trim(),
            verify_same.trim(),
        );

        let detail = format!("System proxy configured via GSettings (SOCKS5 {host}:{port})",);
        Ok(detail)
    }

    fn clear(&mut self) {
        log::info!("[proxy] GSettings: clearing proxy settings");

        run_silent(
            "gsettings",
            &["set", "org.gnome.system.proxy", "mode", "none"],
        );
        run_silent(
            "gsettings",
            &["set", "org.gnome.system.proxy", "use-same-proxy", "true"],
        );

        let (_, verify_mode) =
            run_silent_with_output("gsettings", &["get", "org.gnome.system.proxy", "mode"]);
        log::info!("[proxy] GSettings cleared — mode={}", verify_mode.trim(),);
    }
}

#[cfg(target_os = "linux")]
pub struct KdeProxy;

#[cfg(target_os = "linux")]
impl KdeProxy {
    pub fn is_available() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        desktop.split(':').any(|d| d == "KDE" || d == "Trinity")
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

    fn kioslaverc_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("kioslaverc")
    }

    fn notify_kio() {
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
}

#[cfg(target_os = "linux")]
impl ProxyBackend for KdeProxy {
    fn name(&self) -> &str {
        "KDE KIO"
    }

    fn set(&mut self, host: &str, port: u16) -> Result<String, String> {
        let kwriteconfig = Self::kwriteconfig_command();
        let kioslaverc = Self::kioslaverc_path();
        let kioslaverc_str = kioslaverc.to_string_lossy().to_string();
        let proxy_url = format!("socks5://{host}:{port}");

        log::info!(
            "[proxy] KDE: setting SOCKS5 proxy to {proxy_url} \
             (kwriteconfig={kwriteconfig}, kioslaverc={kioslaverc_str})",
        );

        run_silent(
            kwriteconfig,
            &[
                "--file",
                &kioslaverc_str,
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
                &kioslaverc_str,
                "--group",
                "Proxy Settings",
                "--key",
                "socksProxy",
                &proxy_url,
            ],
        );
        for key in &["httpProxy", "httpsProxy", "ftpProxy"] {
            run_silent(
                kwriteconfig,
                &[
                    "--file",
                    &kioslaverc_str,
                    "--group",
                    "Proxy Settings",
                    "--key",
                    key,
                    "",
                ],
            );
        }
        run_silent(
            kwriteconfig,
            &[
                "--file",
                &kioslaverc_str,
                "--group",
                "Proxy Settings",
                "--key",
                "NoProxyFor",
                "localhost,127.0.0.0/8,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16",
            ],
        );

        Self::notify_kio();

        let detail = format!("System proxy configured via KDE KIO (SOCKS5 {host}:{port})",);
        Ok(detail)
    }

    fn clear(&mut self) {
        log::info!("[proxy] KDE: clearing proxy settings");

        let kwriteconfig = Self::kwriteconfig_command();
        let kioslaverc = Self::kioslaverc_path();
        let kioslaverc_str = kioslaverc.to_string_lossy().to_string();

        run_silent(
            kwriteconfig,
            &[
                "--file",
                &kioslaverc_str,
                "--group",
                "Proxy Settings",
                "--key",
                "ProxyType",
                "0",
            ],
        );

        Self::notify_kio();

        log::info!("[proxy] KDE: proxy type reset to 0");
    }
}
