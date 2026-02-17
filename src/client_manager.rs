use std::{
    cmp::Reverse,
    collections::HashMap,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use futures::AsyncReadExt;
use gpui::http_client::{self, AsyncBody, HttpClient, HttpRequestExt, RedirectPolicy};
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct ClientRelease {
    pub tag: String,
    pub asset_url: String,
    pub asset_name: String,
    pub asset_size: u64,
}

pub struct ClientManagerState {
    pub releases: Vec<ClientRelease>,
    pub installed: Vec<String>,
    pub selected_version: Option<String>,
    pub downloads: HashMap<String, u8>,
    pub fetching_releases: bool,
    pub releases_fetched: bool,
    pub http_client: Arc<dyn HttpClient>,
}

impl ClientManagerState {
    pub fn new(http_client: Arc<dyn HttpClient>) -> Self {
        Self {
            releases: Vec::new(),
            installed: Vec::new(),
            selected_version: None,
            downloads: HashMap::new(),
            fetching_releases: false,
            releases_fetched: false,
            http_client,
        }
    }

    pub fn is_downloading(&self, tag: &str) -> bool {
        self.downloads.contains_key(tag)
    }

    pub fn has_selected_client(&self) -> bool {
        if let Some(ref version) = self.selected_version {
            self.installed.contains(version)
        } else {
            false
        }
    }

    pub fn selected_binary_path(&self) -> Option<PathBuf> {
        let version = self.selected_version.as_ref()?;
        if !self.installed.contains(version) {
            return None;
        }
        Some(client_binary_path(version))
    }
}

pub fn clients_directory() -> PathBuf {
    let directory = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("trusttunnel")
        .join("clients");
    if let Err(error) = std::fs::create_dir_all(&directory) {
        log::warn!(
            "[client_manager] failed to create clients directory {}: {error}",
            directory.display()
        );
    }
    directory
}

pub fn client_binary_path(version: &str) -> PathBuf {
    let binary_name = if cfg!(target_os = "windows") {
        "trusttunnel_client.exe"
    } else {
        "trusttunnel_client"
    };
    clients_directory().join(version).join(binary_name)
}

fn platform_asset_filter() -> (&'static str, &'static str) {
    let operating_system = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };

    let architecture = if cfg!(target_os = "macos") {
        "universal"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "arm") {
        "armv7"
    } else {
        "x86_64"
    };

    (operating_system, architecture)
}

fn find_platform_asset(assets: &[GitHubAsset]) -> Option<(String, String, u64)> {
    let (operating_system, architecture) = platform_asset_filter();
    let pattern = format!("-{operating_system}-{architecture}.");

    for asset in assets {
        if asset.name.contains(&pattern) {
            return Some((
                asset.name.clone(),
                asset.browser_download_url.clone(),
                asset.size,
            ));
        }
    }
    None
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

pub fn fetch_releases_blocking(
    http_client: &Arc<dyn HttpClient>,
) -> Result<Vec<ClientRelease>, String> {
    log::info!("[client_manager] fetching releases from GitHub");

    let url = "https://api.github.com/repos/TrustTunnel/TrustTunnelClient/releases";

    let request = http_client::Request::get(url)
        .header("Accept", "application/vnd.github+json")
        .follow_redirects(RedirectPolicy::FollowAll)
        .body(AsyncBody::empty())
        .map_err(|error| format!("Failed to build request: {error}"))?;

    let mut response = futures::executor::block_on(http_client.send(request))
        .map_err(|error| format!("HTTP request failed: {error}"))?;

    let status = response.status();
    let mut body = Vec::new();
    futures::executor::block_on(response.body_mut().read_to_end(&mut body))
        .map_err(|error| format!("Failed to read response body: {error}"))?;

    if !status.is_success() {
        let text = String::from_utf8_lossy(&body);
        return Err(format!(
            "GitHub API returned status {}: {}",
            status.as_u16(),
            text.chars().take(200).collect::<String>()
        ));
    }

    let github_releases: Vec<GitHubRelease> = serde_json::from_slice(&body)
        .map_err(|error| format!("Failed to parse releases JSON: {error}"))?;

    let mut releases = Vec::new();
    for release in &github_releases {
        if release.draft || release.prerelease {
            continue;
        }
        if let Some((asset_name, asset_url, asset_size)) = find_platform_asset(&release.assets) {
            releases.push(ClientRelease {
                tag: release.tag_name.clone(),
                asset_url,
                asset_name,
                asset_size,
            });
        }
    }

    log::info!(
        "[client_manager] found {} releases with platform assets",
        releases.len()
    );
    Ok(releases)
}

pub fn download_release_blocking(
    release: &ClientRelease,
    state: &Arc<Mutex<ClientManagerState>>,
    http_client: &Arc<dyn HttpClient>,
) -> Result<(), String> {
    let version = &release.tag;
    let version_directory = clients_directory().join(version);

    if let Err(error) = std::fs::create_dir_all(&version_directory) {
        return Err(format!(
            "Failed to create directory {}: {error}",
            version_directory.display()
        ));
    }

    let archive_path = version_directory.join(&release.asset_name);

    log::info!(
        "[client_manager] downloading {} → {} (expected {} bytes)",
        release.asset_url,
        archive_path.display(),
        release.asset_size,
    );

    let request = http_client::Request::get(&release.asset_url)
        .header("Accept", "application/octet-stream")
        .follow_redirects(RedirectPolicy::FollowAll)
        .body(AsyncBody::empty())
        .map_err(|error| format!("Failed to build download request: {error}"))?;

    let mut response = futures::executor::block_on(http_client.send(request))
        .map_err(|error| format!("Download request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("Download failed with status {}", status.as_u16()));
    }

    let expected = release.asset_size;

    let download_result: Result<(), String> = futures::executor::block_on(async {
        let mut file = std::fs::File::create(&archive_path)
            .map_err(|error| format!("Failed to create archive file: {error}"))?;

        let body = response.body_mut();
        let mut received: u64 = 0;
        let mut buffer = vec![0u8; 32768];

        loop {
            let bytes_read = body
                .read(&mut buffer)
                .await
                .map_err(|error| format!("Failed to read response body: {error}"))?;

            if bytes_read == 0 {
                break;
            }

            file.write_all(&buffer[..bytes_read])
                .map_err(|error| format!("Failed to write to archive file: {error}"))?;

            received += bytes_read as u64;

            if expected > 0 {
                let percent = ((received as f64 / expected as f64) * 100.0).min(99.0) as u8;
                if let Ok(mut locked) = state.lock() {
                    locked.downloads.insert(version.to_string(), percent);
                }
            }
        }

        file.flush()
            .map_err(|error| format!("Failed to flush archive file: {error}"))?;

        Ok(())
    });

    if let Err(error) = download_result {
        let _ = std::fs::remove_file(&archive_path);
        return Err(error);
    }

    if let Ok(mut locked) = state.lock() {
        locked.downloads.insert(version.to_string(), 100);
    }

    if !archive_path.exists() {
        return Err("Download completed but archive file not found".into());
    }

    log::info!(
        "[client_manager] extracting {} in {}",
        archive_path.display(),
        version_directory.display()
    );

    extract_archive(&archive_path, &version_directory)?;
    let _ = std::fs::remove_file(&archive_path);

    let binary_path = client_binary_path(version);
    if !binary_path.exists() {
        if let Some(found) = find_binary_recursive(&version_directory) {
            if found != binary_path {
                if let Err(error) = std::fs::rename(&found, &binary_path) {
                    return Err(format!(
                        "Found binary at {} but failed to move: {error}",
                        found.display()
                    ));
                }
                cleanup_empty_directories(&version_directory, &binary_path);
            }
        } else {
            let _ = std::fs::remove_dir_all(&version_directory);
            return Err("Extraction completed but client binary not found in archive".into());
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) =
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
        {
            log::warn!("[client_manager] failed to set executable permission: {error}");
        }
    }

    log::info!(
        "[client_manager] successfully installed {} at {}",
        version,
        binary_path.display()
    );
    Ok(())
}

pub fn remove_client(version: &str) -> Result<(), String> {
    let version_directory = clients_directory().join(version);
    if version_directory.exists() {
        std::fs::remove_dir_all(&version_directory).map_err(|error| {
            format!("Failed to remove {}: {error}", version_directory.display())
        })?;
        log::info!("[client_manager] removed client version {version}");
    }
    Ok(())
}

pub fn scan_installed_clients() -> Vec<String> {
    let directory = clients_directory();
    let mut installed = Vec::new();

    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(_) => return installed,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if !name.starts_with('v') {
                continue;
            }
            let binary = client_binary_path(name);
            if binary.exists() {
                installed.push(name.to_string());
            }
        }
    }

    installed.sort_by_key(|b| Reverse(version_sort_key(b)));
    log::info!(
        "[client_manager] found {} installed client versions",
        installed.len()
    );
    installed
}

pub fn start_fetch_releases(state: Arc<Mutex<ClientManagerState>>) {
    let http_client;
    {
        let mut locked = state.lock().unwrap();
        if locked.releases_fetched || locked.fetching_releases {
            return;
        }
        locked.fetching_releases = true;
        http_client = locked.http_client.clone();
    }

    std::thread::spawn(move || {
        let result = fetch_releases_blocking(&http_client);
        let mut locked = state.lock().unwrap();
        locked.fetching_releases = false;
        match result {
            Ok(releases) => {
                locked.releases = releases;
                locked.releases_fetched = true;
                log::info!(
                    "[client_manager] fetched {} releases",
                    locked.releases.len()
                );
            }
            Err(error) => {
                locked.releases_fetched = true;
                log::error!("[client_manager] fetch releases failed: {error}");
            }
        }
    });
}

pub fn start_download(state: Arc<Mutex<ClientManagerState>>, release: ClientRelease) {
    let version = release.tag.clone();

    let http_client;
    {
        let mut locked = state.lock().unwrap();
        if locked.is_downloading(&version) {
            return;
        }
        locked.downloads.insert(version.clone(), 0);
        http_client = locked.http_client.clone();
    }

    std::thread::spawn(move || {
        let download_result = download_release_blocking(&release, &state, &http_client);

        let mut locked = state.lock().unwrap();
        locked.downloads.remove(&version);
        match download_result {
            Ok(()) => {
                locked.installed = scan_installed_clients();

                if !locked.has_selected_client() {
                    locked.selected_version = Some(version.clone());
                    log::info!("[client_manager] auto-selected version {version}");
                }

                log::info!("[client_manager] download complete: {version}");
            }
            Err(error) => {
                log::error!("[client_manager] download failed for {version}: {error}");
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn extract_archive(
    archive_path: &std::path::Path,
    target_directory: &std::path::Path,
) -> Result<(), String> {
    let output = std::process::Command::new("tar")
        .args(["xzf"])
        .arg(archive_path)
        .arg("-C")
        .arg(target_directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|error| format!("Failed to run tar: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "tar extraction failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn extract_archive(
    archive_path: &std::path::Path,
    target_directory: &std::path::Path,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let archive_str = archive_path.to_string_lossy();

    if archive_str.ends_with(".zip") {
        let script = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            archive_path.display(),
            target_directory.display()
        );
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000)
            .output()
            .map_err(|error| format!("Failed to run powershell: {error}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Zip extraction failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ));
        }
    } else {
        let mut command = std::process::Command::new("tar");
        command
            .args(["xzf"])
            .arg(archive_path)
            .arg("-C")
            .arg(target_directory)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        command.creation_flags(0x08000000);

        let output = command
            .output()
            .map_err(|error| format!("Failed to run tar: {error}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "tar extraction failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ));
        }
    }

    Ok(())
}

fn find_binary_recursive(directory: &std::path::Path) -> Option<PathBuf> {
    let binary_name = if cfg!(target_os = "windows") {
        "trusttunnel_client.exe"
    } else {
        "trusttunnel_client"
    };

    let entries = std::fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name == binary_name
        {
            return Some(path);
        } else if path.is_dir()
            && let Some(found) = find_binary_recursive(&path)
        {
            return Some(found);
        }
    }
    None
}

fn cleanup_empty_directories(base_directory: &std::path::Path, keep_path: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(base_directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path != keep_path.parent().unwrap_or(base_directory) {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
}

fn version_sort_key(tag: &str) -> Vec<u64> {
    tag.trim_start_matches('v')
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}
