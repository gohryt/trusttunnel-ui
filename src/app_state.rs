use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::configuration::{StoredCredential, TunnelMode};

#[derive(Serialize, Deserialize)]
pub struct AppState {
    #[serde(default)]
    pub credential_order: Vec<String>,
    #[serde(default)]
    pub selected_credential: Option<String>,
    #[serde(default)]
    pub tunnel_mode: Option<String>,
    #[serde(default = "default_dns_enabled")]
    pub dns_enabled: bool,
    #[serde(default)]
    pub selected_client_version: Option<String>,
}

fn default_dns_enabled() -> bool {
    true
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            credential_order: Vec::new(),
            selected_credential: None,
            tunnel_mode: None,
            dns_enabled: true,
            selected_client_version: None,
        }
    }
}

impl AppState {
    pub fn state_file_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("trusttunnel")
            .join("trusttunnel-ui.toml")
    }

    pub fn load() -> Self {
        let path = Self::state_file_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(state) => {
                    log::info!("[app_state] loaded from {}", path.display());
                    state
                }
                Err(error) => {
                    log::warn!("[app_state] failed to parse {}: {error}", path.display());
                    Self::default()
                }
            },
            Err(_) => {
                log::info!(
                    "[app_state] no state file at {}, using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let path = Self::state_file_path();
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            log::warn!(
                "[app_state] failed to create state directory {}: {error}",
                parent.display()
            );
        }
        match toml::to_string_pretty(self) {
            Ok(content) => {
                if let Err(error) = std::fs::write(&path, content) {
                    log::warn!("[app_state] failed to write {}: {error}", path.display());
                }
            }
            Err(error) => {
                log::warn!("[app_state] failed to serialize state: {error}");
            }
        }
    }

    pub fn dns_enabled(&self) -> bool {
        self.dns_enabled
    }

    pub fn selected_client_version(&self) -> Option<&str> {
        self.selected_client_version.as_deref()
    }

    pub fn set_selected_client_version(&mut self, version: Option<&str>) {
        self.selected_client_version = version.map(|v| v.to_string());
    }

    pub fn set_dns_enabled(&mut self, enabled: bool) {
        self.dns_enabled = enabled;
    }

    pub fn tunnel_mode(&self) -> TunnelMode {
        match self.tunnel_mode.as_deref() {
            Some("tun") => TunnelMode::Tun,
            Some("system_proxy") => TunnelMode::SystemProxy,
            Some("proxy") => TunnelMode::Proxy,
            _ => TunnelMode::Tun,
        }
    }

    pub fn set_tunnel_mode(&mut self, mode: TunnelMode) {
        self.tunnel_mode = Some(
            match mode {
                TunnelMode::Tun => "tun",
                TunnelMode::SystemProxy => "system_proxy",
                TunnelMode::Proxy => "proxy",
            }
            .to_string(),
        );
    }

    pub fn set_credential_order(&mut self, credentials: &[StoredCredential]) {
        self.credential_order = credentials
            .iter()
            .map(|credential| credential.path.to_string_lossy().to_string())
            .collect();
    }

    pub fn set_selected_credential(&mut self, credential: Option<&StoredCredential>) {
        self.selected_credential =
            credential.map(|stored| stored.path.to_string_lossy().to_string());
    }

    pub fn find_selected_index(&self, credentials: &[StoredCredential]) -> Option<usize> {
        let selected_path = self.selected_credential.as_ref()?;
        credentials
            .iter()
            .position(|credential| credential.path.to_string_lossy() == *selected_path)
    }
}

pub fn apply_saved_order(credentials: &mut Vec<StoredCredential>, saved_order: &[String]) {
    if saved_order.is_empty() {
        return;
    }

    let mut ordered = Vec::with_capacity(credentials.len());
    let mut remaining = std::mem::take(credentials);

    for saved_path in saved_order {
        let path = Path::new(saved_path);
        if let Some(position) = remaining
            .iter()
            .position(|credential| credential.path == path)
        {
            ordered.push(remaining.remove(position));
        }
    }

    ordered.append(&mut remaining);
    *credentials = ordered;
}
