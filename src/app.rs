use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use gpui::{
    App, AsyncApp, Context, CursorStyle, Entity, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollAnchor, ScrollHandle, WeakEntity,
    Window, actions, div, prelude::*, px, rgb,
};

use crate::{
    app_state::AppState, components::*, configuration::*, log_panel::LogPanel, state::*, system::*,
    text_area::TextArea, text_input::TextInput, theme::*,
};

actions!(
    trusttunnel,
    [
        Connect,
        Disconnect,
        FocusNext,
        FocusPrevious,
        Activate,
        Quit,
        AddCredential,
        ImportCredential,
        RemoveCredential
    ]
);

pub struct CredentialDragState {
    pub index: usize,
    pub start_y: Pixels,
    pub moved: bool,
}

pub struct AppInitialization {
    pub hostname_input: Entity<TextInput>,
    pub addresses_input: Entity<TextInput>,
    pub username_input: Entity<TextInput>,
    pub password_input: Entity<TextInput>,
    pub certificate_input: Entity<TextArea>,
    pub dns_upstreams_input: Entity<TextInput>,
    pub has_ipv6: bool,
    pub skip_verification: bool,
    pub upstream_protocol: String,
    pub upstream_fallback_protocol: String,
    pub anti_dpi: bool,
    pub killswitch_enabled: bool,
    pub post_quantum_group_enabled: bool,
    pub dns_enabled: bool,
    pub configuration_path: PathBuf,
    pub log_panel: Entity<LogPanel>,
    pub binary_path: String,
    pub binary_found: bool,
    pub stored_credentials: Vec<StoredCredential>,
    pub selected_credential: Option<usize>,
    pub tunnel_mode: TunnelMode,
}

const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

fn session_timestamp() -> String {
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = duration.as_secs();
    let time_of_day = total_seconds % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let days = (total_seconds / 86400) as i64;
    let adjusted_days = days + 719468;
    let era = (if adjusted_days >= 0 {
        adjusted_days
    } else {
        adjusted_days - 146096
    }) / 146097;
    let day_of_era = (adjusted_days - era * 146097) as u32;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_offset = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_offset + 2) / 5 + 1;
    let month = if month_offset < 10 {
        month_offset + 3
    } else {
        month_offset - 9
    };
    let year = if month <= 2 { year + 1 } else { year };

    format!("{year:04}-{month:02}-{day:02}_{hours:02}-{minutes:02}-{seconds:02}")
}

pub struct TrustTunnelApp {
    hostname_input: Entity<TextInput>,
    addresses_input: Entity<TextInput>,
    username_input: Entity<TextInput>,
    password_input: Entity<TextInput>,
    certificate_input: Entity<TextArea>,
    dns_upstreams_input: Entity<TextInput>,
    has_ipv6: bool,
    skip_verification: bool,
    upstream_protocol: String,
    upstream_fallback_protocol: String,
    anti_dpi: bool,
    killswitch_enabled: bool,
    post_quantum_group_enabled: bool,
    dns_enabled: bool,
    tunnel_mode: TunnelMode,
    connection_state: ConnectionState,
    child_process: Option<Child>,
    status_detail: String,
    configuration_path: PathBuf,
    focus_handle: FocusHandle,
    ipv6_focus_handle: FocusHandle,
    skip_verification_focus_handle: FocusHandle,
    anti_dpi_focus_handle: FocusHandle,
    upstream_http2_focus_handle: FocusHandle,
    upstream_http3_focus_handle: FocusHandle,
    fallback_none_focus_handle: FocusHandle,
    fallback_http2_focus_handle: FocusHandle,
    fallback_http3_focus_handle: FocusHandle,
    mode_tun_focus_handle: FocusHandle,
    mode_system_proxy_focus_handle: FocusHandle,
    mode_proxy_focus_handle: FocusHandle,
    dns_enabled_focus_handle: FocusHandle,
    killswitch_focus_handle: FocusHandle,
    post_quantum_focus_handle: FocusHandle,
    connect_button_focus_handle: FocusHandle,
    credential_focus_handles: Vec<FocusHandle>,
    import_focus_handle: FocusHandle,
    add_focus_handle: FocusHandle,
    remove_focus_handle: FocusHandle,
    process_log: Arc<Mutex<ProcessLog>>,
    log_panel: Entity<LogPanel>,
    log_scroll_handle: ScrollHandle,
    configuration_scroll_handle: ScrollHandle,
    configuration_scroll_anchors: [ScrollAnchor; 10],
    log_file: Option<Arc<Mutex<fs::File>>>,
    system_proxy_set: bool,
    resolvconf_set: bool,
    binary_path: String,
    binary_found: bool,
    poll_tick: u32,
    disconnecting_since: Option<Instant>,
    stored_credentials: Vec<StoredCredential>,
    selected_credential: Option<usize>,
    credential_drag: Option<CredentialDragState>,
}

impl TrustTunnelApp {
    pub fn new(initialization: AppInitialization, context: &mut Context<Self>) -> Self {
        let mut stored_credentials = initialization.stored_credentials;
        let mut selected_credential = initialization.selected_credential;

        if stored_credentials.is_empty() {
            stored_credentials.push(StoredCredential::new_draft(&credentials_directory(), &[]));
            selected_credential = Some(0);
        }

        let credential_count = stored_credentials.len();

        let configuration_scroll_handle = ScrollHandle::new();
        let configuration_scroll_anchors =
            std::array::from_fn(|_| ScrollAnchor::for_handle(configuration_scroll_handle.clone()));

        Self {
            hostname_input: initialization.hostname_input,
            addresses_input: initialization.addresses_input,
            username_input: initialization.username_input,
            password_input: initialization.password_input,
            certificate_input: initialization.certificate_input,
            dns_upstreams_input: initialization.dns_upstreams_input,
            has_ipv6: initialization.has_ipv6,
            skip_verification: initialization.skip_verification,
            upstream_protocol: initialization.upstream_protocol,
            upstream_fallback_protocol: initialization.upstream_fallback_protocol,
            anti_dpi: initialization.anti_dpi,
            killswitch_enabled: initialization.killswitch_enabled,
            post_quantum_group_enabled: initialization.post_quantum_group_enabled,
            dns_enabled: initialization.dns_enabled,
            tunnel_mode: initialization.tunnel_mode,
            connection_state: ConnectionState::Disconnected,
            child_process: None,
            status_detail: String::new(),
            configuration_path: initialization.configuration_path,
            focus_handle: context.focus_handle(),
            ipv6_focus_handle: context.focus_handle(),
            skip_verification_focus_handle: context.focus_handle(),
            anti_dpi_focus_handle: context.focus_handle(),
            upstream_http2_focus_handle: context.focus_handle(),
            upstream_http3_focus_handle: context.focus_handle(),
            fallback_none_focus_handle: context.focus_handle(),
            fallback_http2_focus_handle: context.focus_handle(),
            fallback_http3_focus_handle: context.focus_handle(),
            mode_tun_focus_handle: context.focus_handle(),
            mode_system_proxy_focus_handle: context.focus_handle(),
            mode_proxy_focus_handle: context.focus_handle(),
            dns_enabled_focus_handle: context.focus_handle(),
            killswitch_focus_handle: context.focus_handle(),
            post_quantum_focus_handle: context.focus_handle(),
            connect_button_focus_handle: context.focus_handle(),
            credential_focus_handles: (0..credential_count)
                .map(|_| context.focus_handle())
                .collect(),
            import_focus_handle: context.focus_handle(),
            add_focus_handle: context.focus_handle(),
            remove_focus_handle: context.focus_handle(),
            process_log: Arc::new(Mutex::new(ProcessLog::new())),
            log_panel: initialization.log_panel,
            log_scroll_handle: ScrollHandle::new(),
            configuration_scroll_handle,
            configuration_scroll_anchors,
            log_file: None,
            system_proxy_set: false,
            resolvconf_set: false,
            binary_path: initialization.binary_path,
            binary_found: initialization.binary_found,
            poll_tick: 0,
            disconnecting_since: None,
            stored_credentials,
            selected_credential,
            credential_drag: None,
        }
    }

    pub fn hostname_input(&self, context: &gpui::App) -> FocusHandle {
        self.hostname_input.read(context).focus_handle.clone()
    }

    pub fn configuration_directory() -> PathBuf {
        let directory = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("trusttunnel");
        if let Err(error) = std::fs::create_dir_all(&directory) {
            log::warn!(
                "[configuration] failed to create configuration directory {}: {error}",
                directory.display()
            );
        }
        directory
    }

    fn cleanup_child(&mut self) -> Option<Child> {
        if self.system_proxy_set {
            log::info!("[cleanup] restoring system proxy");
            clear_system_proxy();
            self.system_proxy_set = false;
        }
        if self.resolvconf_set {
            log::info!("[cleanup] restoring DNS via resolvconf");
            clear_dns_resolvconf();
            self.resolvconf_set = false;
        }
        self.child_process.take()
    }

    fn create_session_log_file(&self) -> Option<Arc<Mutex<fs::File>>> {
        let credential_name = self
            .selected_credential
            .and_then(|index| self.stored_credentials.get(index))
            .map(|stored| stored.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("unknown");

        let sanitized_name: String = credential_name
            .chars()
            .map(|character| match character {
                '/' | '\\' | '\0' => '_',
                _ => character,
            })
            .collect();

        let logs_directory = credentials_directory().join("logs").join(&sanitized_name);

        if let Err(error) = fs::create_dir_all(&logs_directory) {
            log::warn!("[logs] failed to create log directory: {error}");
            return None;
        }

        let timestamp = session_timestamp();
        let log_path = logs_directory.join(format!("{timestamp}.log"));

        match fs::File::create(&log_path) {
            Ok(file) => {
                log::info!("[logs] session log: {}", log_path.display());
                Some(Arc::new(Mutex::new(file)))
            }
            Err(error) => {
                log::warn!("[logs] failed to create log file: {error}");
                None
            }
        }
    }

    fn send_terminate_signal(child: &Child) {
        let pid_string = child.id().to_string();
        if run_silent("kill", &["-INT", &pid_string]) {
            log::info!("[terminate] sent SIGINT to pid={pid_string}");
        } else {
            log::info!("[terminate] SIGINT failed for pid={pid_string}, trying pkexec");
            std::thread::spawn(move || {
                run_silent("pkexec", &["kill", "-INT", &pid_string]);
            });
        }
    }

    fn kill_child_background(mut child: Child) {
        let pid_string = child.id().to_string();
        std::thread::spawn(move || {
            if run_silent("kill", &["-INT", &pid_string]) {
                log::info!("[terminate] sent SIGINT to pid={pid_string}");
            } else {
                log::info!("[terminate] SIGINT failed for pid={pid_string}, trying pkexec");
                run_silent("pkexec", &["kill", "-INT", &pid_string]);
            }

            let poll_interval = Duration::from_millis(100);
            let poll_count = GRACEFUL_SHUTDOWN_TIMEOUT.as_millis() / poll_interval.as_millis();

            for attempt in 0..poll_count {
                if let Ok(Some(status)) = child.try_wait() {
                    log::info!(
                        "[terminate] child exited gracefully (attempt {attempt}, status={status})"
                    );
                    return;
                }
                std::thread::sleep(poll_interval);
            }

            log::warn!("[terminate] graceful shutdown timed out for pid={pid_string}");
            if child.kill().is_err() {
                run_silent("pkexec", &["kill", "-KILL", &pid_string]);
            }
            match child.wait() {
                Ok(status) => log::info!("[terminate] child reaped: {status}"),
                Err(error) => log::warn!("[terminate] child wait error: {error}"),
            }
        });
    }

    fn start_log_reader(&self, child: &mut Child) {
        let log_file = self.log_file.clone();

        if let Some(stderr) = child.stderr.take() {
            let shared_log = self.process_log.clone();
            let log_file = log_file.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            if let Some(ref log_file) = log_file
                                && let Ok(mut file) = log_file.lock()
                                && let Err(error) = writeln!(file, "{line}")
                            {
                                log::warn!("[logs] failed to write stderr line: {error}");
                            }
                            let lower = line.to_lowercase();
                            let Ok(mut locked_log) = shared_log.lock() else {
                                break;
                            };

                            if lower.contains("successfully connected to endpoint")
                                || lower.contains("successfully connected")
                                || lower.contains("socks listener started")
                                || (lower.contains("listening") && lower.contains("socks"))
                                || (lower.contains("socks") && lower.contains("bind"))
                            {
                                log::info!("[detect] connection confirmed: {line}");
                                locked_log.connected = true;
                            }

                            if !locked_log.connected
                                && !lower.contains("waiting recovery")
                                && (lower.starts_with("error:")
                                    || lower.contains("failed to")
                                    || lower.contains("denied")
                                    || lower.contains("unauthorized")
                                    || lower.contains("refused")
                                    || lower.contains("failed parsing")
                                    || lower.contains("failed to start listening")
                                    || lower.contains("failed to create listener")
                                    || lower.contains("failed to initialize tunnel")
                                    || lower.contains("couldn't detect active network")
                                    || lower.contains("failed on create vpn"))
                            {
                                log::warn!("[detect] connect-phase error: {line}");
                                locked_log.error = Some(line.clone());
                            }

                            if locked_log.connected
                                && locked_log.post_connect_error.is_none()
                                && (lower.contains("health check error")
                                    || lower.contains("response: http/2.0 407")
                                    || (lower.contains("authorization required")
                                        && !lower.contains("proxy-authenticate"))
                                    || (lower.contains("connection failed")
                                        && lower.contains("socks")))
                            {
                                log::warn!("[detect] post-connect error: {line}");
                                locked_log.post_connect_error = Some(line.clone());
                            }

                            locked_log.lines.push(line);
                            if locked_log.lines.len() > 500 {
                                locked_log.lines.remove(0);
                            }
                        }
                        Err(error) => {
                            log::trace!("[child stderr] reader ended: {error}");
                            break;
                        }
                    }
                }
            });
        } else {
            log::warn!("[log_reader] no stderr pipe from child");
        }

        if let Some(stdout) = child.stdout.take() {
            let shared_log = self.process_log.clone();
            let log_file = log_file.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            if let Some(ref log_file) = log_file
                                && let Ok(mut file) = log_file.lock()
                                && let Err(error) = writeln!(file, "{line}")
                            {
                                log::warn!("[logs] failed to write stdout line: {error}");
                            }
                            let lower = line.to_lowercase();
                            let Ok(mut locked_log) = shared_log.lock() else {
                                break;
                            };
                            if lower.contains("successfully connected to endpoint")
                                || lower.contains("successfully connected")
                            {
                                log::info!("[detect] connection confirmed (stdout): {line}");
                                locked_log.connected = true;
                            }
                            locked_log.lines.push(line);
                            if locked_log.lines.len() > 500 {
                                locked_log.lines.remove(0);
                            }
                        }
                        Err(error) => {
                            log::trace!("[child stdout] reader ended: {error}");
                            break;
                        }
                    }
                }
            });
        }
    }

    fn poll_process_state(&mut self, context: &mut Context<Self>) {
        self.poll_tick = self.poll_tick.wrapping_add(1);
        if !self.poll_tick.is_multiple_of(4) {
            if self.connection_state.is_active() {
                context.notify();
            }
            return;
        }

        if let Some(status) = self.try_reap_child() {
            self.handle_child_exit(status, context);
            return;
        }

        match self.connection_state {
            ConnectionState::Connecting => self.poll_connecting(context),
            ConnectionState::Connected => self.poll_connected(context),
            ConnectionState::Disconnecting => self.poll_disconnecting(context),
            _ => {}
        }
    }

    fn try_reap_child(&mut self) -> Option<std::process::ExitStatus> {
        if let Some(ref mut child) = self.child_process
            && let Ok(Some(status)) = child.try_wait()
        {
            log::debug!("[poll] child exited with status: {status}");
            return Some(status);
        }
        None
    }

    fn handle_child_exit(&mut self, status: std::process::ExitStatus, context: &mut Context<Self>) {
        self.child_process = None;
        if self.system_proxy_set {
            log::info!("[poll] restoring system proxy after client exit");
            clear_system_proxy();
            self.system_proxy_set = false;
        }
        if self.resolvconf_set {
            log::info!("[poll] restoring DNS via resolvconf after client exit");
            clear_dns_resolvconf();
            self.resolvconf_set = false;
        }
        if matches!(self.connection_state, ConnectionState::Disconnecting) {
            log::info!("[poll] child exited during disconnect: {status}");
            self.connection_state = ConnectionState::Disconnected;
            self.status_detail = String::new();
            context.notify();
            return;
        }

        if status.success() {
            self.connection_state = ConnectionState::Disconnected;
            self.status_detail = String::new();
        } else {
            let code = status
                .code()
                .map(|exit_code| exit_code.to_string())
                .unwrap_or_else(|| "signal".into());

            let detail_message = if status.code() == Some(126) {
                "pkexec authentication was dismissed — try again and authenticate when prompted"
                    .to_string()
            } else if status.code() == Some(127) {
                format!(
                    "Binary '{}' not found. Install TrustTunnel client:\n  \
                     https://github.com/TrustTunnel/TrustTunnelClient",
                    self.binary_path,
                )
            } else {
                format!("Client exited with code {code}")
            };

            log::warn!("[poll] {detail_message}");
            self.connection_state = ConnectionState::Error(format!("Exited ({code})"));
            self.status_detail = detail_message;
        }
        context.notify();
    }

    fn poll_connecting(&mut self, context: &mut Context<Self>) {
        let Ok(locked_log) = self.process_log.lock() else {
            return;
        };

        if locked_log.error.is_some() {
            drop(locked_log);

            if let Some(child) = self.cleanup_child() {
                Self::kill_child_background(child);
            }

            self.connection_state = ConnectionState::Error("Connection failed".into());
            self.status_detail = String::new();
            context.notify();
            return;
        }

        if locked_log.connected {
            drop(locked_log);
            self.transition_to_connected(context);
            return;
        }

        drop(locked_log);
        self.status_detail = String::new();
        context.notify();
    }

    fn transition_to_connected(&mut self, context: &mut Context<Self>) {
        self.connection_state = ConnectionState::Connected;

        let mut proxy_detail = String::new();
        if self.tunnel_mode.sets_system_proxy() && !self.system_proxy_set {
            let (host, port) = parse_host_port(PROXY_LISTEN_ADDRESS);
            proxy_detail = set_system_proxy(&host, port);
            self.system_proxy_set = true;
        }

        if self.dns_enabled
            && self.tunnel_mode.is_tun()
            && !check_systemd_resolved()
            && !self.resolvconf_set
            && check_resolvconf()
        {
            let dns_detail = set_dns_resolvconf();
            self.resolvconf_set = true;
            proxy_detail = dns_detail;
        }

        self.status_detail = match self.tunnel_mode {
            TunnelMode::Tun => {
                if self.resolvconf_set {
                    "TUN tunnel active (system-wide)\nDNS configured via resolvconf".into()
                } else {
                    "TUN tunnel active (system-wide)".into()
                }
            }
            TunnelMode::SystemProxy => {
                let mut lines = format!(
                    "System proxy active — all apps route through VPN\n\
                     SOCKS5 on {PROXY_LISTEN_ADDRESS}"
                );
                if !proxy_detail.is_empty() {
                    lines.push_str(&format!("\n{proxy_detail}"));
                }
                lines.push_str("\nProxy will be restored on disconnect or quit");
                lines
            }
            TunnelMode::Proxy => {
                format!(
                    "SOCKS5 proxy on {PROXY_LISTEN_ADDRESS}\n\
                     Terminal: export ALL_PROXY=\"socks5://{PROXY_LISTEN_ADDRESS}\"\n\
                     Firefox: Settings → Network → SOCKS5 Host: 127.0.0.1  Port: 1080\n\
                     Chromium: --proxy-server=\"socks5://{PROXY_LISTEN_ADDRESS}\""
                )
            }
        };
        context.notify();
    }

    fn poll_connected(&mut self, context: &mut Context<Self>) {
        context.notify();
    }

    fn poll_disconnecting(&mut self, context: &mut Context<Self>) {
        let timed_out = self
            .disconnecting_since
            .is_some_and(|since| since.elapsed() >= GRACEFUL_SHUTDOWN_TIMEOUT);

        if !timed_out {
            context.notify();
            return;
        }

        log::warn!("[poll] disconnect timeout, forcing kill");
        if let Some(child) = self.child_process.take() {
            Self::kill_child_background(child);
        }
        self.connection_state = ConnectionState::Disconnected;
        self.status_detail = "Force disconnected (process did not exit in time)".into();
        context.notify();
    }

    fn remove_credential(
        &mut self,
        _: &RemoveCredential,
        _window: &mut Window,
        context: &mut Context<Self>,
    ) {
        if self.is_locked() {
            return;
        }
        let Some(index) = self.selected_credential else {
            return;
        };
        let stored = &self.stored_credentials[index];
        log::info!("[credentials] removing: {}", stored.path.display());

        if !stored.draft
            && let Err(error) = std::fs::remove_file(&stored.path)
        {
            log::warn!("[credentials] failed to remove: {error}");
            self.status_detail = format!("Failed to remove credential: {error}");
            context.notify();
            return;
        }

        self.stored_credentials.remove(index);
        self.credential_focus_handles.remove(index);

        if self.stored_credentials.is_empty() {
            self.stored_credentials
                .push(StoredCredential::new_draft(&credentials_directory(), &[]));
            self.selected_credential = Some(0);
            self.load_credential(&CredentialFile::default(), context);
        } else {
            self.selected_credential = Some(index.min(self.stored_credentials.len() - 1));
            let credential = self.stored_credentials[self.selected_credential.unwrap()]
                .credential
                .clone();
            self.load_credential(&credential, context);
        }

        self.sync_credential_focus_handles(context);
        self.save_app_state();
        context.notify();
    }

    fn import_credential(
        &mut self,
        _: &ImportCredential,
        _window: &mut Window,
        context: &mut Context<Self>,
    ) {
        if self.connection_state.is_active() {
            return;
        }

        self.save_draft_credential(context);

        let executor = context.background_executor().clone();
        let credentials_path = credentials_directory();

        context.spawn(async move |this: WeakEntity<Self>, context: &mut AsyncApp| {
            let path = executor
                .spawn(async move { open_file_dialog("Add Credentials", "TOML files", "*.toml") })
                .await;

            let Some(path) = path else {
                return;
            };

            log::info!("[credentials] adding file: {}", path.display());

            let destination = match add_credential_file(&path, &credentials_path) {
                Ok(destination) => destination,
                Err(error) => {
                    if let Err(update_error) = this.update(context, |this, context| {
                        this.status_detail = error;
                        context.notify();
                    }) {
                        log::warn!("[credentials] failed to update state after import error: {update_error}");
                    }
                    return;
                }
            };

            if let Err(update_error) = this.update(context, |this, context| {
                let saved_state = AppState::load();
                this.stored_credentials = scan_credentials(&credentials_path);
                crate::app_state::apply_saved_order(
                    &mut this.stored_credentials,
                    &saved_state.credential_order,
                );
                let found_index = this
                    .stored_credentials
                    .iter()
                    .position(|entry| entry.path == destination);
                if let Some(selected_index) = found_index {
                    this.select_credential(selected_index, context);
                }
                this.sync_credential_focus_handles(context);
                this.save_app_state();
                context.notify();
            }) {
                log::warn!("[credentials] failed to update state after import: {update_error}");
            }
        })
        .detach();
    }

    fn add_credential(
        &mut self,
        _: &AddCredential,
        _window: &mut Window,
        context: &mut Context<Self>,
    ) {
        if self.is_locked() {
            return;
        }
        self.save_draft_credential(context);
        let draft = StoredCredential::new_draft(&credentials_directory(), &self.stored_credentials);
        self.stored_credentials.push(draft);
        let index = self.stored_credentials.len() - 1;
        self.selected_credential = Some(index);
        self.load_credential(&CredentialFile::default(), context);
        self.sync_credential_focus_handles(context);
        self.save_app_state();
        context.notify();
    }

    fn select_credential(&mut self, index: usize, context: &mut Context<Self>) {
        if index >= self.stored_credentials.len() {
            return;
        }
        self.save_draft_credential(context);
        self.selected_credential = Some(index);
        let credential = self.stored_credentials[index].credential.clone();
        self.load_credential(&credential, context);
        self.save_app_state();
    }

    fn load_credential(&mut self, credential: &CredentialFile, context: &mut Context<Self>) {
        self.set_input(&self.hostname_input.clone(), &credential.hostname, context);
        self.set_input(
            &self.addresses_input.clone(),
            &credential.addresses.join(", "),
            context,
        );
        self.set_input(&self.username_input.clone(), &credential.username, context);
        self.set_input(&self.password_input.clone(), &credential.password, context);
        self.set_input(
            &self.dns_upstreams_input.clone(),
            &credential.dns_upstreams.join(", "),
            context,
        );
        let certificate = credential.certificate.trim().to_string();
        self.certificate_input
            .update(context, |area, _| area.set_content(&certificate));
        self.has_ipv6 = credential.has_ipv6;
        self.skip_verification = credential.skip_verification;
        self.anti_dpi = credential.anti_dpi;
        self.killswitch_enabled = credential.killswitch_enabled;
        self.post_quantum_group_enabled = credential.post_quantum_group_enabled;
        self.upstream_protocol = if credential.upstream_protocol.is_empty() {
            "http2".into()
        } else {
            credential.upstream_protocol.clone()
        };
        self.upstream_fallback_protocol = credential.upstream_fallback_protocol.clone();
        context.notify();
    }

    fn set_input(&self, input: &Entity<TextInput>, value: &str, context: &mut Context<Self>) {
        let content = value.to_string();
        input.update(context, |input, _| {
            input.content = content.into();
            let length = input.content.len();
            input.selected_range = length..length;
        });
    }

    fn is_locked(&self) -> bool {
        self.connection_state.is_active()
    }

    fn sync_credential_focus_handles(&mut self, context: &mut Context<Self>) {
        while self.credential_focus_handles.len() < self.stored_credentials.len() {
            self.credential_focus_handles.push(context.focus_handle());
        }
        self.credential_focus_handles
            .truncate(self.stored_credentials.len());
    }

    fn toggle_has_ipv6(&mut self, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.has_ipv6 = !self.has_ipv6;
            context.notify();
        }
    }

    fn toggle_skip_verification(&mut self, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.skip_verification = !self.skip_verification;
            context.notify();
        }
    }

    fn toggle_anti_dpi(&mut self, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.anti_dpi = !self.anti_dpi;
            context.notify();
        }
    }

    fn toggle_killswitch_enabled(&mut self, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.killswitch_enabled = !self.killswitch_enabled;
            context.notify();
        }
    }

    fn toggle_post_quantum_group_enabled(&mut self, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.post_quantum_group_enabled = !self.post_quantum_group_enabled;
            context.notify();
        }
    }

    fn toggle_dns_enabled(&mut self, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.dns_enabled = !self.dns_enabled;
            self.save_app_state();
            context.notify();
        }
    }

    fn set_upstream_protocol(&mut self, value: &str, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.upstream_protocol = value.into();
            if self.upstream_fallback_protocol == value {
                self.upstream_fallback_protocol = String::new();
            }
            context.notify();
        }
    }

    fn set_fallback_protocol(&mut self, value: &str, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.upstream_fallback_protocol = value.into();
            context.notify();
        }
    }

    fn set_tunnel_mode(&mut self, mode: TunnelMode, context: &mut Context<Self>) {
        if !self.is_locked() {
            self.tunnel_mode = mode;
            self.save_app_state();
            context.notify();
        }
    }

    fn save_app_state(&self) {
        let mut state = AppState::default();
        state.set_credential_order(&self.stored_credentials);
        state.set_tunnel_mode(self.tunnel_mode);
        state.set_dns_enabled(self.dns_enabled);
        state.set_selected_credential(
            self.selected_credential
                .and_then(|index| self.stored_credentials.get(index)),
        );
        state.save();
    }

    fn build_credential_from_fields(&self, context: &App) -> CredentialFile {
        let hostname = self.hostname_input.read(context).text().trim().to_string();
        let addresses_raw = self.addresses_input.read(context).text().trim().to_string();
        let addresses: Vec<String> = addresses_raw
            .split([',', ' '])
            .map(|segment| segment.trim().to_string())
            .filter(|segment| !segment.is_empty())
            .collect();
        let username = self.username_input.read(context).text().trim().to_string();
        let password = self.password_input.read(context).text();
        let certificate = self
            .certificate_input
            .read(context)
            .text()
            .trim()
            .to_string();
        let dns_upstreams_raw = self
            .dns_upstreams_input
            .read(context)
            .text()
            .trim()
            .to_string();
        let dns_upstreams: Vec<String> = dns_upstreams_raw
            .split([',', ' '])
            .map(|segment| segment.trim().to_string())
            .filter(|segment| !segment.is_empty())
            .collect();

        CredentialFile {
            hostname,
            addresses,
            has_ipv6: self.has_ipv6,
            username,
            password,
            skip_verification: self.skip_verification,
            certificate,
            upstream_protocol: self.upstream_protocol.clone(),
            upstream_fallback_protocol: self.upstream_fallback_protocol.clone(),
            anti_dpi: self.anti_dpi,
            killswitch_enabled: self.killswitch_enabled,
            post_quantum_group_enabled: self.post_quantum_group_enabled,
            dns_upstreams,
        }
    }

    fn save_draft_credential(&mut self, context: &App) {
        let Some(index) = self.selected_credential else {
            return;
        };
        if index >= self.stored_credentials.len() || !self.stored_credentials[index].draft {
            return;
        }

        let credential = self.build_credential_from_fields(context);
        if credential.hostname.is_empty() && credential.username.is_empty() {
            return;
        }

        self.stored_credentials[index].credential = credential;
        if let Err(error) = self.stored_credentials[index].save_to_disk() {
            log::warn!("[credentials] failed to save draft: {error}");
            return;
        }
        log::info!(
            "[credentials] draft saved: {}",
            self.stored_credentials[index].name,
        );
    }

    fn start_credential_drag(&mut self, index: usize, event: &MouseDownEvent) {
        self.credential_drag = Some(CredentialDragState {
            index,
            start_y: event.position.y,
            moved: false,
        });
    }

    fn update_credential_drag(&mut self, event: &MouseMoveEvent, context: &mut Context<Self>) {
        let Some(drag) = self.credential_drag.as_mut() else {
            return;
        };

        let item_stride = px(ELEMENT_HEIGHT + GAP_EXTRA_SMALL);
        let delta = event.position.y - drag.start_y;

        if delta > item_stride / 2.0 && drag.index + 1 < self.stored_credentials.len() {
            self.stored_credentials.swap(drag.index, drag.index + 1);
            self.credential_focus_handles
                .swap(drag.index, drag.index + 1);
            if self.selected_credential == Some(drag.index) {
                self.selected_credential = Some(drag.index + 1);
            } else if self.selected_credential == Some(drag.index + 1) {
                self.selected_credential = Some(drag.index);
            }
            drag.index += 1;
            drag.start_y += item_stride;
            drag.moved = true;
            context.notify();
        } else if delta < -item_stride / 2.0 && drag.index > 0 {
            self.stored_credentials.swap(drag.index, drag.index - 1);
            self.credential_focus_handles
                .swap(drag.index, drag.index - 1);
            if self.selected_credential == Some(drag.index) {
                self.selected_credential = Some(drag.index - 1);
            } else if self.selected_credential == Some(drag.index - 1) {
                self.selected_credential = Some(drag.index);
            }
            drag.index -= 1;
            drag.start_y -= item_stride;
            drag.moved = true;
            context.notify();
        }
    }

    fn end_credential_drag(&mut self) -> bool {
        if let Some(drag) = self.credential_drag.take() {
            if drag.moved {
                self.save_app_state();
            }
            return drag.moved;
        }
        false
    }

    fn connect(&mut self, _: &Connect, _window: &mut Window, context: &mut Context<Self>) {
        if self.is_locked() {
            return;
        }

        self.save_draft_credential(context);

        let mode = self.tunnel_mode;
        log::info!("━━━ CONNECT (mode={}) ━━━", mode.label());

        if !self.binary_found {
            self.connection_state = ConnectionState::Error("Client binary not found".into());
            self.status_detail =
                "Could not find 'trusttunnel_client' in PATH or standard locations.\n\n\
                 Install the TrustTunnel client:\n  \
                 https://github.com/TrustTunnel/TrustTunnelClient"
                    .to_string();
            context.notify();
            return;
        }

        if mode.is_tun() && !check_tun_device() {
            self.connection_state = ConnectionState::Error("TUN device not available".into());
            self.status_detail = "/dev/net/tun not found. Load the tun kernel module:\n  \
                 sudo modprobe tun\n\n\
                 To make it persistent, add 'tun' to /etc/modules-load.d/tun.conf"
                .into();
            context.notify();
            return;
        }

        if mode.is_tun() && !check_pkexec() {
            self.connection_state = ConnectionState::Error("pkexec not found".into());
            self.status_detail = "pkexec is required for TUN mode (root privileges needed).\n\
                 Install policykit-1 or try Proxy/System Proxy mode instead."
                .into();
            context.notify();
            return;
        }

        if let Some(child) = self.cleanup_child() {
            Self::kill_child_background(child);
        }

        let hostname = self.hostname_input.read(context).text().trim().to_string();
        let addresses_raw = self.addresses_input.read(context).text().trim().to_string();
        let username = self.username_input.read(context).text().trim().to_string();
        let password = self.password_input.read(context).text();
        let certificate = self
            .certificate_input
            .read(context)
            .text()
            .trim()
            .to_string();
        let dns_upstreams_raw = self
            .dns_upstreams_input
            .read(context)
            .text()
            .trim()
            .to_string();
        let dns_upstreams: Vec<String> = dns_upstreams_raw
            .split([',', ' '])
            .map(|segment| segment.trim().to_string())
            .filter(|segment| !segment.is_empty())
            .collect();

        if let Some(error) = Self::validate_fields(&hostname, &addresses_raw, &username, &password)
        {
            self.connection_state = ConnectionState::Error(error.0.clone());
            self.status_detail = error.1.clone();
            context.notify();
            return;
        }

        let addresses: Vec<String> = addresses_raw
            .split([',', ' '])
            .map(|segment| segment.trim().to_string())
            .filter(|segment| !segment.is_empty())
            .map(|address| {
                if address.contains(':') {
                    address
                } else {
                    format!("{address}:443")
                }
            })
            .collect();

        let mut configuration = VpnConfiguration::new(
            EndpointFields {
                hostname,
                addresses,
                has_ipv6: self.has_ipv6,
                username,
                password,
                skip_verification: self.skip_verification,
                certificate,
                upstream_protocol: self.upstream_protocol.clone(),
                upstream_fallback_protocol: self.upstream_fallback_protocol.clone(),
                anti_dpi: self.anti_dpi,
                killswitch_enabled: self.killswitch_enabled,
                post_quantum_group_enabled: self.post_quantum_group_enabled,
                dns_enabled: self.dns_enabled,
                dns_upstreams,
            },
            mode,
        );

        if mode.is_tun()
            && let Some(ref mut tun) = configuration.listener.tun
        {
            for endpoint_address in &configuration.endpoint.addresses {
                let host = endpoint_address
                    .split(':')
                    .next()
                    .unwrap_or(endpoint_address);
                if let Ok(address) = host.parse::<std::net::IpAddr>() {
                    let route = match address {
                        std::net::IpAddr::V4(ipv4) => format!("{ipv4}/32"),
                        std::net::IpAddr::V6(ipv6) => format!("{ipv6}/128"),
                    };
                    if !tun.excluded_routes.contains(&route) {
                        tun.excluded_routes.push(route);
                    }
                }
            }
        }

        let toml_string = match toml::to_string_pretty(&configuration) {
            Ok(value) => value,
            Err(error) => {
                let message = format!("Configuration serialization error: {error}");
                self.connection_state = ConnectionState::Error(message.clone());
                self.status_detail = message;
                context.notify();
                return;
            }
        };

        log::info!(
            "[connect] generated configuration:\n{}",
            redact_password_in_toml(&toml_string),
        );

        if let Err(error) = std::fs::write(&self.configuration_path, &toml_string) {
            let message = format!("Failed to write config: {error}");
            self.connection_state = ConnectionState::Error(message.clone());
            self.status_detail = message;
            context.notify();
            return;
        }

        self.log_file = self.create_session_log_file();
        if let Ok(mut locked_log) = self.process_log.lock() {
            locked_log.reset();
        }
        self.connection_state = ConnectionState::Connecting;
        context.notify();

        let binary = &self.binary_path;
        let configuration_path = &self.configuration_path;

        let spawn_result = if mode.is_tun() {
            log::info!(
                "[connect] spawning: pkexec {} -c {}",
                binary,
                configuration_path.display(),
            );
            Command::new("pkexec")
                .arg(binary)
                .arg("-c")
                .arg(configuration_path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        } else {
            log::info!(
                "[connect] spawning: {} -c {}",
                binary,
                configuration_path.display(),
            );
            Command::new(binary)
                .arg("-c")
                .arg(configuration_path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };

        match spawn_result {
            Ok(mut child) => {
                log::info!(
                    "[connect] child started (pid={}, mode={})",
                    child.id(),
                    mode.label(),
                );
                self.start_log_reader(&mut child);
                self.child_process = Some(child);
                self.status_detail = String::new();
            }
            Err(error) => {
                self.connection_state = ConnectionState::Error("Failed to start client".into());
                self.status_detail = format!(
                    "Could not start TrustTunnel client: {error}\n\n\
                     Install the TrustTunnel client:\n  \
                     https://github.com/TrustTunnel/TrustTunnelClient"
                );
            }
        }
        context.notify();
    }

    fn validate_fields(
        hostname: &str,
        addresses: &str,
        username: &str,
        password: &str,
    ) -> Option<(String, String)> {
        if addresses.is_empty() {
            return Some((
                "Addresses required".into(),
                "Enter at least one endpoint address (e.g. 78.141.223.149:443)".into(),
            ));
        }
        if hostname.is_empty() {
            return Some((
                "Hostname is required".into(),
                "Enter the endpoint hostname (e.g. vpn.example.com)".into(),
            ));
        }
        if username.is_empty() {
            return Some(("Username is required".into(), "Enter your username".into()));
        }
        if password.is_empty() {
            return Some(("Password is required".into(), "Enter your password".into()));
        }
        None
    }

    fn disconnect(&mut self, _: &Disconnect, _window: &mut Window, context: &mut Context<Self>) {
        if matches!(self.connection_state, ConnectionState::Disconnecting) {
            return;
        }
        log::info!("━━━ DISCONNECT ━━━");

        if self.system_proxy_set {
            log::info!("[disconnect] restoring system proxy");
            clear_system_proxy();
            self.system_proxy_set = false;
        }
        if self.resolvconf_set {
            log::info!("[disconnect] restoring DNS via resolvconf");
            clear_dns_resolvconf();
            self.resolvconf_set = false;
        }

        if let Some(ref mut child) = self.child_process {
            if let Ok(Some(status)) = child.try_wait() {
                log::info!("[disconnect] child already exited: {status}");
                self.child_process = None;
                self.connection_state = ConnectionState::Disconnected;
                self.status_detail = format!("Client already exited ({status})");
                context.notify();
                return;
            }

            self.connection_state = ConnectionState::Disconnecting;
            self.status_detail = String::new();
            self.disconnecting_since = Some(Instant::now());
            Self::send_terminate_signal(child);
            context.notify();
        } else {
            self.connection_state = ConnectionState::Disconnected;
            self.status_detail = String::new();
            context.notify();
        }
    }

    fn on_connect_click(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        if self.connection_state.is_busy() {
            return;
        }
        if self.connection_state.is_connected() {
            self.disconnect(&Disconnect, window, context);
        } else {
            self.connect(&Connect, window, context);
        }
    }

    fn activate(&mut self, _: &Activate, window: &mut Window, context: &mut Context<Self>) {
        if let Some(index) = self
            .credential_focus_handles
            .iter()
            .position(|handle| handle.is_focused(window))
        {
            if !self.is_locked() {
                self.select_credential(index, context);
                context.notify();
            }
            return;
        }
        if self.import_focus_handle.is_focused(window) {
            self.import_credential(&ImportCredential, window, context);
            return;
        }
        if self.add_focus_handle.is_focused(window) {
            self.add_credential(&AddCredential, window, context);
            return;
        }
        if self.remove_focus_handle.is_focused(window) {
            self.remove_credential(&RemoveCredential, window, context);
            return;
        }
        if self.ipv6_focus_handle.is_focused(window) {
            self.toggle_has_ipv6(context);
        } else if self.skip_verification_focus_handle.is_focused(window) {
            self.toggle_skip_verification(context);
        } else if self.anti_dpi_focus_handle.is_focused(window) {
            self.toggle_anti_dpi(context);
        } else if self.upstream_http2_focus_handle.is_focused(window) {
            self.set_upstream_protocol("http2", context);
        } else if self.upstream_http3_focus_handle.is_focused(window) {
            self.set_upstream_protocol("http3", context);
        } else if self.fallback_none_focus_handle.is_focused(window) {
            self.set_fallback_protocol("", context);
        } else if self.fallback_http2_focus_handle.is_focused(window) {
            self.set_fallback_protocol("http2", context);
        } else if self.fallback_http3_focus_handle.is_focused(window) {
            self.set_fallback_protocol("http3", context);
        } else if self.mode_tun_focus_handle.is_focused(window) {
            self.set_tunnel_mode(TunnelMode::Tun, context);
        } else if self.mode_system_proxy_focus_handle.is_focused(window) {
            self.set_tunnel_mode(TunnelMode::SystemProxy, context);
        } else if self.mode_proxy_focus_handle.is_focused(window) {
            self.set_tunnel_mode(TunnelMode::Proxy, context);
        } else if self.dns_enabled_focus_handle.is_focused(window) {
            self.toggle_dns_enabled(context);
        } else if self.killswitch_focus_handle.is_focused(window) {
            self.toggle_killswitch_enabled(context);
        } else if self.post_quantum_focus_handle.is_focused(window) {
            self.toggle_post_quantum_group_enabled(context);
        } else if self.connect_button_focus_handle.is_focused(window)
            && !self.connection_state.is_busy()
        {
            if self.connection_state.is_connected() {
                self.disconnect(&Disconnect, window, context);
            } else {
                self.connect(&Connect, window, context);
            }
        }
    }

    fn focusable_entries(&self, context: &App) -> Vec<(FocusHandle, Option<ScrollAnchor>)> {
        let anchors = &self.configuration_scroll_anchors;
        let mut entries: Vec<(FocusHandle, Option<ScrollAnchor>)> = self
            .credential_focus_handles
            .iter()
            .map(|handle| (handle.clone(), Some(anchors[9].clone())))
            .collect();
        entries.extend([
            (self.import_focus_handle.clone(), Some(anchors[9].clone())),
            (self.add_focus_handle.clone(), Some(anchors[9].clone())),
            (self.remove_focus_handle.clone(), Some(anchors[9].clone())),
        ]);
        entries.extend([
            (
                self.hostname_input.read(context).focus_handle.clone(),
                Some(anchors[0].clone()),
            ),
            (
                self.addresses_input.read(context).focus_handle.clone(),
                Some(anchors[1].clone()),
            ),
            (
                self.username_input.read(context).focus_handle.clone(),
                Some(anchors[2].clone()),
            ),
            (
                self.password_input.read(context).focus_handle.clone(),
                Some(anchors[3].clone()),
            ),
            (
                self.certificate_input.read(context).focus_handle.clone(),
                Some(anchors[4].clone()),
            ),
            (
                self.dns_upstreams_input.read(context).focus_handle.clone(),
                Some(anchors[5].clone()),
            ),
            (self.ipv6_focus_handle.clone(), Some(anchors[6].clone())),
            (
                self.skip_verification_focus_handle.clone(),
                Some(anchors[6].clone()),
            ),
            (self.anti_dpi_focus_handle.clone(), Some(anchors[6].clone())),
            (
                self.upstream_http2_focus_handle.clone(),
                Some(anchors[7].clone()),
            ),
            (
                self.upstream_http3_focus_handle.clone(),
                Some(anchors[7].clone()),
            ),
            (
                self.fallback_none_focus_handle.clone(),
                Some(anchors[8].clone()),
            ),
        ]);
        if self.upstream_protocol != "http2" {
            entries.push((
                self.fallback_http2_focus_handle.clone(),
                Some(anchors[8].clone()),
            ));
        }
        if self.upstream_protocol != "http3" {
            entries.push((
                self.fallback_http3_focus_handle.clone(),
                Some(anchors[8].clone()),
            ));
        }
        entries.extend([
            (self.mode_tun_focus_handle.clone(), None),
            (self.mode_system_proxy_focus_handle.clone(), None),
            (self.mode_proxy_focus_handle.clone(), None),
            (self.dns_enabled_focus_handle.clone(), None),
            (self.killswitch_focus_handle.clone(), None),
            (self.post_quantum_focus_handle.clone(), None),
            (self.connect_button_focus_handle.clone(), None),
            (self.log_panel.focus_handle(context), None),
        ]);
        entries
    }

    fn focus_entry(
        entries: &[(FocusHandle, Option<ScrollAnchor>)],
        index: usize,
        window: &mut Window,
        context: &mut App,
    ) {
        let (handle, scroll_anchor) = &entries[index];
        window.focus(handle, context);
        if let Some(anchor) = scroll_anchor {
            anchor.scroll_to(window, context);
        }
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, context: &mut Context<Self>) {
        let entries = self.focusable_entries(context);
        for (current, (handle, _)) in entries.iter().enumerate() {
            if handle.is_focused(window) {
                let next = (current + 1) % entries.len();
                Self::focus_entry(&entries, next, window, context);
                return;
            }
        }
        Self::focus_entry(&entries, 0, window, context);
    }

    fn focus_previous(
        &mut self,
        _: &FocusPrevious,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let entries = self.focusable_entries(context);
        for (current, (handle, _)) in entries.iter().enumerate() {
            if handle.is_focused(window) {
                let previous = if current == 0 {
                    entries.len() - 1
                } else {
                    current - 1
                };
                Self::focus_entry(&entries, previous, window, context);
                return;
            }
        }
        Self::focus_entry(&entries, 0, window, context);
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, context: &mut Context<Self>) {
        log::info!("[quit] shutting down");
        self.save_draft_credential(context);
        if let Some(child) = self.cleanup_child() {
            Self::kill_child_background(child);
        }
        context.quit();
    }
}

impl Drop for TrustTunnelApp {
    fn drop(&mut self) {
        log::info!("[drop] TrustTunnelApp shutting down");
        if let Some(child) = self.cleanup_child() {
            Self::kill_child_background(child);
        }
    }
}

impl Focusable for TrustTunnelApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TrustTunnelApp {
    fn render(&mut self, _window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        if self.connection_state.is_active() {
            self.poll_process_state(context);
        }

        let state_label_text = self.connection_state.label();
        let state_color = self.connection_state.color();
        let detail = self.status_detail.clone();
        let tunnel_mode = self.tunnel_mode;
        let locked = self.is_locked();
        let busy = self.connection_state.is_busy();

        {
            let lines: Vec<String> = self
                .process_log
                .lock()
                .map(|locked_log| locked_log.lines.clone())
                .unwrap_or_default();
            let changed = self
                .log_panel
                .update(context, |panel, _| panel.set_lines(&lines));
            if changed {
                self.log_scroll_handle.scroll_to_bottom();
            }
        }

        self.hostname_input
            .update(context, |input, _| input.disabled = locked);
        self.addresses_input
            .update(context, |input, _| input.disabled = locked);
        self.username_input
            .update(context, |input, _| input.disabled = locked);
        self.password_input
            .update(context, |input, _| input.disabled = locked);
        self.dns_upstreams_input
            .update(context, |input, _| input.disabled = locked);
        self.certificate_input
            .update(context, |area, _| area.disabled = locked);

        let (button_label, button_background, button_hover_background) = match self.connection_state
        {
            ConnectionState::Connected | ConnectionState::Disconnecting => {
                ("Disconnect", BUTTON_DANGER, BUTTON_DANGER_HOVER)
            }
            _ => ("Connect", BUTTON_PRIMARY, BUTTON_HOVER),
        };

        let upstream = self.upstream_protocol.clone();
        let fallback = self.upstream_fallback_protocol.clone();

        div()
            .key_context("TrustTunnelApp")
            .track_focus(&self.focus_handle(context))
            .on_action(context.listener(Self::connect))
            .on_action(context.listener(Self::disconnect))
            .on_action(context.listener(Self::focus_next))
            .on_action(context.listener(Self::focus_previous))
            .on_action(context.listener(Self::activate))
            .on_action(context.listener(Self::quit))
            .on_action(context.listener(Self::add_credential))
            .on_action(context.listener(Self::import_credential))
            .on_action(context.listener(Self::remove_credential))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(SURFACE))
            .child(self.render_titlebar(context))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("configuration-scroll")
                            .flex()
                            .flex_col()
                            .w(px(LEFT_COLUMN_WIDTH))
                            .flex_shrink_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.configuration_scroll_handle)
                            .border_r_1()
                            .border_color(rgb(BORDER))
                            .px(px(PADDING_COLUMN))
                            .pb(px(PADDING_COLUMN))
                            .pt(px(PADDING_COLUMN_TOP))
                            .gap(px(GAP_MEDIUM))
                            .child(
                                div()
                                    .id("anchor-credentials")
                                    .anchor_scroll(Some(
                                        self.configuration_scroll_anchors[9].clone(),
                                    ))
                                    .child(self.render_credential_list(locked, context)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(GAP_SMALL))
                                    .child(
                                        div()
                                            .id("anchor-hostname")
                                            .anchor_scroll(Some(
                                                self.configuration_scroll_anchors[0].clone(),
                                            ))
                                            .child(field("Hostname", &self.hostname_input)),
                                    )
                                    .child(
                                        div()
                                            .id("anchor-addresses")
                                            .anchor_scroll(Some(
                                                self.configuration_scroll_anchors[1].clone(),
                                            ))
                                            .child(field(
                                                "Addresses (comma-separated)",
                                                &self.addresses_input,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id("anchor-username")
                                            .anchor_scroll(Some(
                                                self.configuration_scroll_anchors[2].clone(),
                                            ))
                                            .child(field("Username", &self.username_input)),
                                    )
                                    .child(
                                        div()
                                            .id("anchor-password")
                                            .anchor_scroll(Some(
                                                self.configuration_scroll_anchors[3].clone(),
                                            ))
                                            .child(field("Password", &self.password_input)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("anchor-certificate")
                                    .anchor_scroll(Some(
                                        self.configuration_scroll_anchors[4].clone(),
                                    ))
                                    .child(self.render_certificate_section()),
                            )
                            .child(
                                div()
                                    .id("anchor-dns-upstreams")
                                    .anchor_scroll(Some(
                                        self.configuration_scroll_anchors[5].clone(),
                                    ))
                                    .child(field(
                                        "DNS Upstreams (comma-separated)",
                                        &self.dns_upstreams_input,
                                    )),
                            )
                            .child(
                                div()
                                    .id("anchor-endpoint-toggles")
                                    .anchor_scroll(Some(
                                        self.configuration_scroll_anchors[6].clone(),
                                    ))
                                    .child(self.render_endpoint_toggles(locked, context)),
                            )
                            .child(
                                div()
                                    .id("anchor-upstream")
                                    .anchor_scroll(Some(
                                        self.configuration_scroll_anchors[7].clone(),
                                    ))
                                    .child(
                                        self.render_upstream_selector(&upstream, locked, context),
                                    ),
                            )
                            .child(
                                div()
                                    .id("anchor-fallback")
                                    .anchor_scroll(Some(
                                        self.configuration_scroll_anchors[8].clone(),
                                    ))
                                    .child(
                                        self.render_fallback_selector(&fallback, locked, context),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_hidden()
                            .px(px(PADDING_COLUMN))
                            .pb(px(PADDING_COLUMN))
                            .pt(px(PADDING_COLUMN_TOP))
                            .gap(px(GAP_MEDIUM))
                            .child(self.render_mode_selector(tunnel_mode, locked, context))
                            .child(self.render_connection_toggles(locked, context))
                            .child(
                                button_action(
                                    button_label,
                                    button_background,
                                    button_hover_background,
                                    busy,
                                    &self.connect_button_focus_handle,
                                )
                                .when(!busy, |element| {
                                    element.on_mouse_up(
                                        MouseButton::Left,
                                        context.listener(Self::on_connect_click),
                                    )
                                }),
                            )
                            .child(self.render_status(state_label_text, state_color, detail))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .overflow_hidden()
                                    .gap(px(GAP_EXTRA_SMALL))
                                    .child(label("Logs"))
                                    .child(
                                        log_container()
                                            .track_scroll(&self.log_scroll_handle)
                                            .child(self.log_panel.clone()),
                                    ),
                            ),
                    ),
            )
    }
}

impl TrustTunnelApp {
    fn render_titlebar(&self, context: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(TITLEBAR_HEIGHT))
            .bg(rgb(TITLEBAR_BACKGROUND))
            .child(
                titlebar_title("TrustTunnel")
                    .cursor(CursorStyle::default())
                    .on_mouse_down(
                        MouseButton::Left,
                        context.listener(|_, _, window, _| window.start_window_move()),
                    ),
            )
            .child(titlebar_close().on_mouse_up(
                MouseButton::Left,
                context.listener(|this, _, window, context| this.quit(&Quit, window, context)),
            ))
    }

    fn render_credential_list(
        &self,
        locked: bool,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_selection = self.selected_credential.is_some();

        div()
            .flex()
            .flex_col()
            .gap(px(GAP_EXTRA_SMALL))
            .w_full()
            .child(label("Credentials"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(GAP_SMALL))
                    .w_full()
                    .child({
                        let dragging_index = self.credential_drag.as_ref().map(|drag| drag.index);
                        let mut list = div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(px(GAP_EXTRA_SMALL))
                            .overflow_hidden()
                            .on_mouse_move(context.listener(
                                |this, event: &MouseMoveEvent, _, context| {
                                    this.update_credential_drag(event, context);
                                },
                            ))
                            .on_mouse_up(
                                MouseButton::Left,
                                context.listener(|this, _, _, _| {
                                    this.end_credential_drag();
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                context.listener(|this, _, _, _| {
                                    this.end_credential_drag();
                                }),
                            );
                        for (credential_index, stored) in self.stored_credentials.iter().enumerate()
                        {
                            let active = self.selected_credential == Some(credential_index);
                            let is_dragged = dragging_index == Some(credential_index);
                            list = list.child(
                                credential_item(
                                    &stored.name,
                                    active,
                                    is_dragged,
                                    locked,
                                    &self.credential_focus_handles[credential_index],
                                )
                                .when(!locked, |element| {
                                    element.on_mouse_down(
                                        MouseButton::Left,
                                        context.listener(
                                            move |this, event: &MouseDownEvent, _, _| {
                                                this.start_credential_drag(credential_index, event);
                                            },
                                        ),
                                    )
                                })
                                .on_mouse_up(
                                    MouseButton::Left,
                                    context.listener(move |this, _, _, context| {
                                        let was_moved = this.end_credential_drag();
                                        if !this.is_locked() && !was_moved {
                                            this.select_credential(credential_index, context);
                                        }
                                    }),
                                ),
                            );
                        }
                        list
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_shrink_0()
                            .w(px(CREDENTIAL_BUTTON_WIDTH))
                            .gap(px(GAP_EXTRA_SMALL))
                            .child(
                                button_ghost("Import", locked, &self.import_focus_handle).when(
                                    !locked,
                                    |element| {
                                        element.on_mouse_up(
                                            MouseButton::Left,
                                            context.listener(|this, _, window, context| {
                                                this.import_credential(
                                                    &ImportCredential,
                                                    window,
                                                    context,
                                                );
                                            }),
                                        )
                                    },
                                ),
                            )
                            .child(button_ghost("Add", locked, &self.add_focus_handle).when(
                                !locked,
                                |element| {
                                    element.on_mouse_up(
                                        MouseButton::Left,
                                        context.listener(|this, _, window, context| {
                                            this.add_credential(&AddCredential, window, context);
                                        }),
                                    )
                                },
                            ))
                            .child(
                                button_ghost("Remove", locked, &self.remove_focus_handle).when(
                                    !locked && has_selection,
                                    |element| {
                                        element.on_mouse_up(
                                            MouseButton::Left,
                                            context.listener(|this, _, window, context| {
                                                this.remove_credential(
                                                    &RemoveCredential,
                                                    window,
                                                    context,
                                                );
                                            }),
                                        )
                                    },
                                ),
                            ),
                    ),
            )
    }

    fn render_certificate_section(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(GAP_EXTRA_SMALL))
            .w_full()
            .child(label("Certificate (PEM)"))
            .child(self.certificate_input.clone())
    }

    fn render_endpoint_toggles(
        &self,
        locked: bool,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(GAP_SMALL))
            .child(toggle(
                "IPv6",
                self.has_ipv6,
                locked,
                &self.ipv6_focus_handle,
                context.listener(|this, _, _, context| this.toggle_has_ipv6(context)),
            ))
            .child(toggle(
                "Skip Verification",
                self.skip_verification,
                locked,
                &self.skip_verification_focus_handle,
                context.listener(|this, _, _, context| this.toggle_skip_verification(context)),
            ))
            .child(toggle(
                "Anti-DPI",
                self.anti_dpi,
                locked,
                &self.anti_dpi_focus_handle,
                context.listener(|this, _, _, context| this.toggle_anti_dpi(context)),
            ))
    }

    fn render_connection_toggles(
        &self,
        locked: bool,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(GAP_SMALL))
            .child(toggle(
                "Override system DNS",
                self.dns_enabled,
                locked,
                &self.dns_enabled_focus_handle,
                context.listener(|this, _, _, context| this.toggle_dns_enabled(context)),
            ))
            .child(toggle(
                "Kill Switch",
                self.killswitch_enabled,
                locked,
                &self.killswitch_focus_handle,
                context.listener(|this, _, _, context| this.toggle_killswitch_enabled(context)),
            ))
            .child(toggle(
                "Post-Quantum",
                self.post_quantum_group_enabled,
                locked,
                &self.post_quantum_focus_handle,
                context.listener(|this, _, _, context| {
                    this.toggle_post_quantum_group_enabled(context)
                }),
            ))
    }

    fn render_upstream_selector(
        &self,
        upstream: &str,
        locked: bool,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        selector(
            "Upstream Protocol",
            selector_row()
                .child(
                    selector_option(
                        "HTTP/2",
                        upstream == "http2",
                        locked,
                        &self.upstream_http2_focus_handle,
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        context.listener(|this, _, _, context| {
                            this.set_upstream_protocol("http2", context);
                        }),
                    ),
                )
                .child(
                    selector_option(
                        "HTTP/3",
                        upstream == "http3",
                        locked,
                        &self.upstream_http3_focus_handle,
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        context.listener(|this, _, _, context| {
                            this.set_upstream_protocol("http3", context);
                        }),
                    ),
                ),
        )
    }

    fn render_fallback_selector(
        &self,
        fallback: &str,
        locked: bool,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let upstream = self.upstream_protocol.as_str();
        let mut row = selector_row().child(
            selector_option(
                "None",
                fallback.is_empty(),
                locked,
                &self.fallback_none_focus_handle,
            )
            .on_mouse_up(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.set_fallback_protocol("", context);
                }),
            ),
        );
        if upstream != "http2" {
            row = row.child(
                selector_option(
                    "HTTP/2",
                    fallback == "http2",
                    locked,
                    &self.fallback_http2_focus_handle,
                )
                .on_mouse_up(
                    MouseButton::Left,
                    context.listener(|this, _, _, context| {
                        this.set_fallback_protocol("http2", context);
                    }),
                ),
            );
        }
        if upstream != "http3" {
            row = row.child(
                selector_option(
                    "HTTP/3",
                    fallback == "http3",
                    locked,
                    &self.fallback_http3_focus_handle,
                )
                .on_mouse_up(
                    MouseButton::Left,
                    context.listener(|this, _, _, context| {
                        this.set_fallback_protocol("http3", context);
                    }),
                ),
            );
        }
        selector("Fallback Protocol", row)
    }

    fn render_mode_selector(
        &self,
        tunnel_mode: TunnelMode,
        locked: bool,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        selector(
            "Mode",
            selector_row()
                .child(
                    selector_option(
                        "TUN",
                        tunnel_mode == TunnelMode::Tun,
                        locked,
                        &self.mode_tun_focus_handle,
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        context.listener(|this, _, _, context| {
                            this.set_tunnel_mode(TunnelMode::Tun, context);
                        }),
                    ),
                )
                .child(
                    selector_option(
                        "System proxy",
                        tunnel_mode == TunnelMode::SystemProxy,
                        locked,
                        &self.mode_system_proxy_focus_handle,
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        context.listener(|this, _, _, context| {
                            this.set_tunnel_mode(TunnelMode::SystemProxy, context);
                        }),
                    ),
                )
                .child(
                    selector_option(
                        "Proxy",
                        tunnel_mode == TunnelMode::Proxy,
                        locked,
                        &self.mode_proxy_focus_handle,
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        context.listener(|this, _, _, context| {
                            this.set_tunnel_mode(TunnelMode::Proxy, context);
                        }),
                    ),
                ),
        )
    }

    fn render_status(
        &self,
        state_label_text: String,
        state_color: u32,
        detail: String,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(GAP_EXTRA_SMALL))
            .px(px(PADDING_INPUT_HORIZONTAL))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .child(status_dot(state_color))
                    .child(status_label(state_label_text, state_color)),
            )
            .when(!detail.is_empty(), |element| {
                element.child(status_detail(detail))
            })
    }
}
