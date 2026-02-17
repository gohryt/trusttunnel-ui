#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod app_state;
mod client_manager;
mod components;
mod configuration;
mod connection_state;
mod log_panel;
mod process_log;
#[cfg(target_os = "windows")]
mod single_instance;
mod system;
mod text_area;
mod text_input;
mod theme;

use std::sync::{Arc, Mutex};

use gpui::{
    Application, Bounds, KeyBinding, WindowBackgroundAppearance, WindowBounds, WindowDecorations,
    WindowOptions, prelude::*, px, size,
};

use crate::{
    app::{
        Activate, AppInitialization, FocusNext, FocusPrevious, ImportCredential, Quit, SwitchTab,
        SwitchTabPrevious, TrustTunnelApp,
    },
    app_state::{AppState, apply_saved_order},
    client_manager::{ClientManagerState, scan_installed_clients},
    configuration::{
        StoredCredential, add_credential_file, credentials_directory, scan_credentials,
    },
    log_panel::LogPanel,
    text_area::{Down, Enter, SelectDown, SelectUp, TextArea, Up},
    text_input::{
        Backspace, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft,
        SelectRight, TextInput,
    },
    theme::{WINDOW_HEIGHT, WINDOW_WIDTH},
};

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("trusttunnel_ui=info"),
    )
    .init();

    log::info!(
        "trusttunnel-ui {} starting (RUST_LOG={})",
        env!("CARGO_PKG_VERSION"),
        std::env::var("RUST_LOG").unwrap_or_else(|_| "<default: info>".into()),
    );

    let system_services = system::system_services();

    {
        let default_hook = std::panic::take_hook();
        let system_services = system_services.clone();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("[panic] application panicked, performing emergency cleanup");

            system_services.emergency_cleanup();

            default_hook(info);
        }));
    }

    #[cfg(target_os = "windows")]
    let _single_instance_guard = match single_instance::acquire() {
        Some(guard) => guard,
        None => {
            log::warn!("[application_startup] another instance is already running — exiting");
            #[cfg(not(debug_assertions))]
            single_instance::show_already_running_message();
            return;
        }
    };

    #[cfg(target_os = "linux")]
    log::info!(
        "[env] XDG_CURRENT_DESKTOP={}, XDG_SESSION_TYPE={}, DISPLAY={}, WAYLAND_DISPLAY={}",
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        std::env::var("DISPLAY").unwrap_or_default(),
        std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
    );

    #[cfg(target_os = "windows")]
    log::info!(
        "[env] OS={}, COMPUTERNAME={}",
        std::env::var("OS").unwrap_or_default(),
        std::env::var("COMPUTERNAME").unwrap_or_default(),
    );

    system_services.startup_cleanup();

    let saved_state_early = AppState::load();
    let installed_clients = scan_installed_clients();
    let saved_client_version = saved_state_early
        .selected_client_version()
        .map(|v| v.to_string());

    let credentials_path = credentials_directory();
    let initial_credential = load_initial_credentials(&credentials_path);
    let mut stored_credentials = scan_credentials(&credentials_path);
    apply_saved_order(&mut stored_credentials, &saved_state_early.credential_order);
    let saved_tunnel_mode = saved_state_early.tunnel_mode();
    let saved_dns_enabled = saved_state_early.dns_enabled();
    let saved_selected_credential = saved_state_early.find_selected_index(&stored_credentials);
    let system_services = system_services.clone();

    let http_client = reqwest_client::ReqwestClient::user_agent("trusttunnel-ui")
        .expect("failed to create HTTP client");

    Application::new()
        .with_http_client(Arc::new(http_client))
        .run(move |context| {
            let client_manager_state = {
                let mut client_manager_state_value = ClientManagerState::new(context.http_client());
                client_manager_state_value.installed = installed_clients;
                client_manager_state_value.selected_version = saved_client_version;
                Arc::new(Mutex::new(client_manager_state_value))
            };

            let (binary_path, binary_found) = {
                let client_manager_state_guard = client_manager_state.lock().unwrap();
                if let Some(managed_path) = client_manager_state_guard.selected_binary_path() {
                    let path_string = managed_path.to_string_lossy().to_string();
                    let exists = managed_path.exists();
                    log::info!(
                        "[application_startup] using managed client binary: {} (exists={})",
                        path_string,
                        exists
                    );
                    (path_string, exists)
                } else {
                    let (path, found) = system_services.find_client_binary();
                    log::info!(
                        "[application_startup] no managed client selected, system binary: {} (found={})",
                        path,
                        found
                    );
                    (path, found)
                }
            };

            if binary_found
                && let Some(error) = system_services.check_binary_works(&binary_path, false)
            {
                log::warn!("[application_startup] binary check issue: {error}");
            }

            let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), context);

            bind_keys(context);

            let configuration_path = TrustTunnelApp::configuration_directory().join("client.toml");
            log::info!(
                "[application_startup] configuration path: {}",
                configuration_path.display()
            );

            let binary_path_clone = binary_path.clone();
            let initial_credential_snapshot = initial_credential.clone();
            let stored_credentials_snapshot = stored_credentials.clone();
            let saved_selected_index = saved_selected_credential;
            let client_manager_state_shared = client_manager_state.clone();

            let window = context.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    is_resizable: true,
                    is_minimizable: true,
                    window_background: WindowBackgroundAppearance::Opaque,
                    window_min_size: Some(size(px(640.0), px(480.0))),
                    window_decorations: Some(WindowDecorations::Client),
                    app_id: Some("TrustTunnel".into()),
                    ..Default::default()
                },
                |_, context| {
                    let log_panel = context.new(LogPanel::new);

                    let selected_credential = initial_credential_snapshot
                        .as_ref()
                        .and_then(|stored_credential| {
                            stored_credentials_snapshot
                                .iter()
                                .position(|entry| entry.path == stored_credential.path)
                        })
                        .or(saved_selected_index);
                    let active_credential = selected_credential
                        .and_then(|index| stored_credentials_snapshot.get(index));

                    let hostname_initial =
                        active_credential.map(|stored| stored.credential.hostname.as_str());
                    let addresses_initial =
                        active_credential.map(|stored| stored.credential.addresses.join(", "));
                    let username_initial =
                        active_credential.map(|stored| stored.credential.username.as_str());
                    let password_initial =
                        active_credential.map(|stored| stored.credential.password.as_str());
                    let dns_upstreams_initial =
                        active_credential.map(|stored| stored.credential.dns_upstreams.join(", "));

                    let hostname_input =
                        TextInput::new(context, "example.com", false, hostname_initial);
                    let addresses_input = TextInput::new(
                        context,
                        "1.2.3.4:443, 5.6.7.8:443",
                        false,
                        addresses_initial.as_deref(),
                    );
                    let username_input = TextInput::new(context, "gohryt", false, username_initial);
                    let password_input =
                        TextInput::new(context, "I'm lovin' it", true, password_initial);
                    let dns_upstreams_input = TextInput::new(
                        context,
                        "tls://1.1.1.1, tls://1.0.0.1",
                        false,
                        dns_upstreams_initial.as_deref(),
                    );

                    let certificate_initial = active_credential
                        .map(|stored| stored.credential.certificate.trim())
                        .filter(|text| !text.is_empty());
                    let certificate_input =
                        TextArea::new(context, "PEM PUM PAM PUM PUM PAM PAM", certificate_initial);

                    let has_ipv6 = active_credential
                        .map(|stored| stored.credential.has_ipv6)
                        .unwrap_or(true);
                    let skip_verification = active_credential
                        .map(|stored| stored.credential.skip_verification)
                        .unwrap_or(false);
                    let anti_dpi = active_credential
                        .map(|stored| stored.credential.anti_dpi)
                        .unwrap_or(false);
                    let killswitch_enabled = active_credential
                        .map(|stored| stored.credential.killswitch_enabled)
                        .unwrap_or(false);
                    let post_quantum_group_enabled = active_credential
                        .map(|stored| stored.credential.post_quantum_group_enabled)
                        .unwrap_or(true);
                    let upstream_protocol = active_credential
                        .map(|stored| {
                            if stored.credential.upstream_protocol.is_empty() {
                                "http2".into()
                            } else {
                                stored.credential.upstream_protocol.clone()
                            }
                        })
                        .unwrap_or_else(|| "http2".into());
                    let upstream_fallback_protocol = active_credential
                        .map(|stored| stored.credential.upstream_fallback_protocol.clone())
                        .unwrap_or_default();

                    log::info!(
                        "[application_startup] binary={}, found={}",
                        binary_path_clone,
                        binary_found,
                    );

                    context.new(|context| {
                        TrustTunnelApp::new(
                            AppInitialization {
                                hostname_input,
                                addresses_input,
                                username_input,
                                password_input,
                                certificate_input,
                                dns_upstreams_input,
                                has_ipv6,
                                skip_verification,
                                upstream_protocol,
                                upstream_fallback_protocol,
                                anti_dpi,
                                killswitch_enabled,
                                post_quantum_group_enabled,
                                dns_enabled: saved_dns_enabled,
                                configuration_path: configuration_path.clone(),
                                system_services: system_services.clone(),
                                log_panel,
                                binary_path: binary_path_clone,
                                binary_found,
                                stored_credentials: stored_credentials_snapshot.clone(),
                                selected_credential,
                                tunnel_mode: saved_tunnel_mode,
                                client_manager_state: client_manager_state_shared,
                            },
                            context,
                        )
                    })
                },
            );

            match window {
                Ok(window) => {
                    if let Err(error) = window.update(context, |view, window, context| {
                        window.set_window_title("TrustTunnel");

                        window.on_window_should_close(context, |_window, _cx| {
                            log::info!("[close] window close requested by platform");
                            true
                        });

                        let handle = view.hostname_input(context);
                        window.focus(&handle, context);
                        context.activate(true);
                    }) {
                        log::error!("[application_startup] failed to initialize application window: {error}");
                        context.quit();
                        return;
                    }

                    context.on_action(|_: &Quit, context| context.quit());
                }
                Err(error) => {
                    log::error!("[application_startup] failed to open application window: {error}");
                    context.quit();
                }
            }
        });
}

fn load_initial_credentials(credentials_path: &std::path::Path) -> Option<StoredCredential> {
    let path = std::env::args().nth(1)?;
    let source = std::path::PathBuf::from(&path);
    log::info!("[application_startup] loading initial credential file: {path}");

    let destination = match add_credential_file(&source, credentials_path) {
        Ok(destination) => destination,
        Err(error) => {
            log::warn!("[application_startup] failed to add credential file: {error}");
            return None;
        }
    };

    let stored = match StoredCredential::from_path(destination.clone()) {
        Some(stored) => stored,
        None => {
            log::warn!(
                "[application_startup] credential file was copied but parsing failed: {}",
                destination.display()
            );
            return None;
        }
    };

    log::info!(
        "[application_startup] credential file added and selected: {} ({})",
        stored.name,
        stored.path.display()
    );
    Some(stored)
}

fn bind_keys(context: &mut gpui::App) {
    context.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-v", Paste, Some("TextInput")),
        KeyBinding::new("cmd-c", Copy, Some("TextInput")),
        KeyBinding::new("cmd-x", Cut, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
    ]);

    context.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextArea")),
        KeyBinding::new("delete", Delete, Some("TextArea")),
        KeyBinding::new("left", Left, Some("TextArea")),
        KeyBinding::new("right", Right, Some("TextArea")),
        KeyBinding::new("up", Up, Some("TextArea")),
        KeyBinding::new("down", Down, Some("TextArea")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextArea")),
        KeyBinding::new("shift-right", SelectRight, Some("TextArea")),
        KeyBinding::new("shift-up", SelectUp, Some("TextArea")),
        KeyBinding::new("shift-down", SelectDown, Some("TextArea")),
        KeyBinding::new("home", Home, Some("TextArea")),
        KeyBinding::new("end", End, Some("TextArea")),
        KeyBinding::new("enter", Enter, Some("TextArea")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextArea")),
        KeyBinding::new("cmd-v", Paste, Some("TextArea")),
        KeyBinding::new("cmd-c", Copy, Some("TextArea")),
        KeyBinding::new("cmd-x", Cut, Some("TextArea")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextArea")),
        KeyBinding::new("ctrl-v", Paste, Some("TextArea")),
        KeyBinding::new("ctrl-c", Copy, Some("TextArea")),
        KeyBinding::new("ctrl-x", Cut, Some("TextArea")),
    ]);

    context.bind_keys([
        KeyBinding::new("cmd-a", SelectAll, Some("LogPanel")),
        KeyBinding::new("cmd-c", Copy, Some("LogPanel")),
        KeyBinding::new("ctrl-a", SelectAll, Some("LogPanel")),
        KeyBinding::new("ctrl-c", Copy, Some("LogPanel")),
    ]);

    context.bind_keys([
        KeyBinding::new("tab", FocusNext, Some("TrustTunnelApp")),
        KeyBinding::new("shift-tab", FocusPrevious, Some("TrustTunnelApp")),
        KeyBinding::new("ctrl-tab", SwitchTab, Some("TrustTunnelApp")),
        KeyBinding::new("ctrl-shift-tab", SwitchTabPrevious, Some("TrustTunnelApp")),
        KeyBinding::new("space", Activate, Some("TrustTunnelApp")),
        KeyBinding::new("cmd-q", Quit, Some("TrustTunnelApp")),
        KeyBinding::new("ctrl-q", Quit, Some("TrustTunnelApp")),
        KeyBinding::new("cmd-o", ImportCredential, Some("TrustTunnelApp")),
        KeyBinding::new("ctrl-o", ImportCredential, Some("TrustTunnelApp")),
    ]);
}
