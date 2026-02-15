use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::system::check_systemd_resolved;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct CredentialFile {
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub has_ipv6: bool,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub skip_verification: bool,
    #[serde(default)]
    pub certificate: String,
    #[serde(default)]
    pub upstream_protocol: String,
    #[serde(default)]
    pub upstream_fallback_protocol: String,
    #[serde(default)]
    pub anti_dpi: bool,
    #[serde(default)]
    pub killswitch_enabled: bool,
    #[serde(default = "default_post_quantum_group_enabled")]
    pub post_quantum_group_enabled: bool,
    #[serde(default = "default_dns_upstreams")]
    pub dns_upstreams: Vec<String>,
}

fn default_post_quantum_group_enabled() -> bool {
    true
}

fn default_dns_upstreams() -> Vec<String> {
    vec!["tls://1.1.1.1".into(), "tls://1.0.0.1".into()]
}

#[derive(Clone)]
pub struct StoredCredential {
    pub path: PathBuf,
    pub name: String,
    pub credential: CredentialFile,
    pub draft: bool,
}

impl StoredCredential {
    pub fn from_path(path: PathBuf) -> Option<Self> {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                log::warn!("[credentials] failed to read {}: {error}", path.display());
                return None;
            }
        };

        let credential: CredentialFile = match toml::from_str(&content) {
            Ok(credential) => credential,
            Err(error) => {
                log::warn!("[credentials] failed to parse {}: {error}", path.display());
                return None;
            }
        };

        let name = credential_name(&credential, &path);

        Some(Self {
            path,
            name,
            credential,
            draft: false,
        })
    }

    pub fn new_draft(directory: &Path, existing: &[StoredCredential]) -> Self {
        let mut counter = 0u32;
        let mut path = directory.join(".draft.toml");
        while existing.iter().any(|entry| entry.path == path) {
            counter += 1;
            path = directory.join(format!(".draft-{counter}.toml"));
        }

        Self {
            path,
            name: String::new(),
            credential: CredentialFile::default(),
            draft: true,
        }
    }

    pub fn save_to_disk(&mut self) -> Result<(), String> {
        let name = credential_name(&self.credential, &self.path);
        let directory = self
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let path = directory.join(format!("{name}.toml"));

        let content = toml::to_string_pretty(&self.credential)
            .map_err(|error| format!("Failed to serialize credential: {error}"))?;
        std::fs::write(&path, &content)
            .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;

        self.path = path;
        self.name = name;
        self.draft = false;
        Ok(())
    }
}

pub fn credential_name(credential: &CredentialFile, path: &Path) -> String {
    let fallback = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown")
        .to_string();

    match (
        credential.username.is_empty(),
        credential.hostname.is_empty(),
    ) {
        (false, false) => format!("{}@{}", credential.username, credential.hostname),
        (true, false) => credential.hostname.clone(),
        (false, true) => credential.username.clone(),
        (true, true) => fallback,
    }
}

pub fn credentials_directory() -> PathBuf {
    let directory = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("trusttunnel");
    if let Err(error) = std::fs::create_dir_all(&directory) {
        log::warn!(
            "[credentials] failed to create configuration directory {}: {error}",
            directory.display()
        );
    }
    directory
}

pub fn scan_credentials(directory: &Path) -> Vec<StoredCredential> {
    let mut result = Vec::new();

    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            log::warn!(
                "[credentials] failed to read {}: {error}",
                directory.display()
            );
            return result;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            continue;
        }
        let file_name = path.file_name().and_then(|name| name.to_str());
        if matches!(file_name, Some("client.toml" | "trusttunnel-ui.toml")) {
            continue;
        }
        match StoredCredential::from_path(path.clone()) {
            Some(stored) => {
                log::debug!("[credentials] loaded: {} ({})", stored.name, path.display());
                result.push(stored);
            }
            None => {
                log::warn!("[credentials] failed to parse: {}", path.display());
            }
        }
    }

    result.sort_by_key(|credential| credential.name.to_lowercase());
    log::info!("[credentials] found {} stored credentials", result.len());
    result
}

pub fn add_credential_file(source: &Path, directory: &Path) -> Result<PathBuf, String> {
    let content = std::fs::read_to_string(source)
        .map_err(|error| format!("Failed to read {}: {error}", source.display()))?;
    let credential: CredentialFile = toml::from_str(&content)
        .map_err(|error| format!("Failed to parse {}: {error}", source.display()))?;

    let base_name = credential_name(&credential, source);

    let mut destination = directory.join(format!("{base_name}.toml"));
    let mut counter = 1u32;
    while destination.exists() {
        let existing = std::fs::read_to_string(&destination).unwrap_or_default();
        if existing == content {
            log::info!(
                "[credentials] identical file already exists: {}",
                destination.display()
            );
            return Ok(destination);
        }
        destination = directory.join(format!("{base_name}_{counter}.toml"));
        counter += 1;
    }

    std::fs::write(&destination, &content)
        .map_err(|error| format!("Failed to write {}: {error}", destination.display()))?;

    log::info!(
        "[credentials] added {} → {}",
        source.display(),
        destination.display()
    );
    Ok(destination)
}

#[derive(Clone, Copy, PartialEq)]
pub enum TunnelMode {
    Tun,
    SystemProxy,
    Proxy,
}

impl TunnelMode {
    pub fn is_tun(self) -> bool {
        matches!(self, Self::Tun)
    }

    pub fn sets_system_proxy(self) -> bool {
        matches!(self, Self::SystemProxy)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Tun => "TUN",
            Self::SystemProxy => "System proxy",
            Self::Proxy => "Proxy",
        }
    }
}

pub const PROXY_LISTEN_ADDRESS: &str = "127.0.0.1:1080";

#[derive(Serialize)]
pub struct VpnConfiguration {
    pub loglevel: String,
    pub vpn_mode: String,
    pub killswitch_enabled: bool,
    pub killswitch_allow_ports: Vec<u16>,
    pub post_quantum_group_enabled: bool,
    pub exclusions: Vec<String>,
    pub dns_upstreams: Vec<String>,
    pub endpoint: EndpointConfiguration,
    pub listener: ListenerConfiguration,
}

#[derive(Serialize)]
pub struct EndpointConfiguration {
    pub hostname: String,
    pub addresses: Vec<String>,
    pub has_ipv6: bool,
    pub username: String,
    pub password: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub client_random: String,
    pub skip_verification: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub certificate: String,
    pub upstream_protocol: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub upstream_fallback_protocol: String,
    pub anti_dpi: bool,
}

#[derive(Serialize)]
pub struct ListenerConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun: Option<TunConfiguration>,
    #[serde(rename = "socks", skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfiguration>,
}

#[derive(Serialize)]
pub struct TunConfiguration {
    #[serde(rename = "bound_if", skip_serializing_if = "String::is_empty")]
    pub bound_interface: String,
    pub included_routes: Vec<String>,
    pub excluded_routes: Vec<String>,
    pub mtu_size: u32,
    pub change_system_dns: bool,
}

#[derive(Serialize)]
pub struct ProxyConfiguration {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

pub struct EndpointFields {
    pub hostname: String,
    pub addresses: Vec<String>,
    pub has_ipv6: bool,
    pub username: String,
    pub password: String,
    pub skip_verification: bool,
    pub certificate: String,
    pub upstream_protocol: String,
    pub upstream_fallback_protocol: String,
    pub anti_dpi: bool,
    pub killswitch_enabled: bool,
    pub post_quantum_group_enabled: bool,
    pub dns_enabled: bool,
    pub dns_upstreams: Vec<String>,
}

impl VpnConfiguration {
    pub fn new(endpoint: EndpointFields, mode: TunnelMode) -> Self {
        log::debug!(
            "[configuration] hostname={}, addresses={:?}, skip_verification={}, has_ipv6={}, upstream={}, fallback={}, anti_dpi={}",
            endpoint.hostname,
            endpoint.addresses,
            endpoint.skip_verification,
            endpoint.has_ipv6,
            endpoint.upstream_protocol,
            endpoint.upstream_fallback_protocol,
            endpoint.anti_dpi,
        );

        let listener = if mode.is_tun() {
            log::debug!("[configuration] building TUN listener config");
            ListenerConfiguration {
                tun: Some(TunConfiguration {
                    bound_interface: String::new(),
                    included_routes: vec!["0.0.0.0/0".into(), "2000::/3".into()],
                    excluded_routes: vec![
                        "0.0.0.0/8".into(),
                        "10.0.0.0/8".into(),
                        "169.254.0.0/16".into(),
                        "172.16.0.0/12".into(),
                        "192.168.0.0/16".into(),
                        "224.0.0.0/3".into(),
                    ],
                    mtu_size: 1280,
                    change_system_dns: endpoint.dns_enabled && check_systemd_resolved(),
                }),
                proxy: None,
            }
        } else {
            log::debug!("[configuration] building proxy listener config on {PROXY_LISTEN_ADDRESS}");
            ListenerConfiguration {
                tun: None,
                proxy: Some(ProxyConfiguration {
                    address: PROXY_LISTEN_ADDRESS.into(),
                    username: None,
                    password: None,
                }),
            }
        };

        Self {
            loglevel: "info".into(),
            vpn_mode: "general".into(),
            killswitch_enabled: endpoint.killswitch_enabled,
            killswitch_allow_ports: vec![],
            post_quantum_group_enabled: endpoint.post_quantum_group_enabled,
            exclusions: vec![],
            dns_upstreams: endpoint.dns_upstreams,
            endpoint: EndpointConfiguration {
                hostname: endpoint.hostname,
                addresses: endpoint.addresses,
                has_ipv6: endpoint.has_ipv6,
                username: endpoint.username,
                password: endpoint.password,
                client_random: String::new(),
                skip_verification: endpoint.skip_verification,
                certificate: endpoint.certificate,
                upstream_protocol: if endpoint.upstream_protocol.is_empty() {
                    "http2".into()
                } else {
                    endpoint.upstream_protocol
                },
                upstream_fallback_protocol: endpoint.upstream_fallback_protocol,
                anti_dpi: endpoint.anti_dpi,
            },
            listener,
        }
    }
}

pub fn redact_password_in_toml(toml: &str) -> String {
    toml.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("password")
                && let Some(equals_position) = line.find('=')
            {
                let value = line[equals_position + 1..].trim();
                let length = value
                    .strip_prefix('"')
                    .and_then(|stripped| stripped.strip_suffix('"'))
                    .map(|inner| inner.len())
                    .unwrap_or(value.len());
                return format!("{}= \"{}\"", &line[..equals_position], "*".repeat(length));
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
