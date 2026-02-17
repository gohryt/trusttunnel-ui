const MAX_LOG_LINES: usize = 500;

pub enum LogLineEvent {
    Connected,
    ConnectError,
    PostConnectError,
    Normal,
}

pub fn classify_log_line(line: &str, already_connected: bool) -> LogLineEvent {
    let lower = line.to_lowercase();

    if lower.contains("successfully connected to endpoint")
        || lower.contains("successfully connected")
        || lower.contains("socks listener started")
        || (lower.contains("listening") && lower.contains("socks"))
        || (lower.contains("socks") && lower.contains("bind"))
    {
        return LogLineEvent::Connected;
    }

    if !already_connected
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
        return LogLineEvent::ConnectError;
    }

    if already_connected
        && (lower.contains("health check error")
            || lower.contains("response: http/2.0 407")
            || (lower.contains("authorization required") && !lower.contains("proxy-authenticate"))
            || (lower.contains("connection failed") && lower.contains("socks")))
    {
        return LogLineEvent::PostConnectError;
    }

    LogLineEvent::Normal
}

pub struct ProcessLog {
    pub lines: Vec<String>,
    pub connected: bool,
    pub error: Option<String>,
    pub post_connect_error: Option<String>,
}

impl ProcessLog {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            connected: false,
            error: None,
            post_connect_error: None,
        }
    }

    pub fn reset(&mut self) {
        self.lines.clear();
        self.connected = false;
        self.error = None;
        self.post_connect_error = None;
    }

    pub fn push_line(&mut self, line: String) {
        match classify_log_line(&line, self.connected) {
            LogLineEvent::Connected => {
                log::info!("[detect] connection confirmed: {line}");
                self.connected = true;
            }
            LogLineEvent::ConnectError => {
                log::warn!("[detect] connect-phase error: {line}");
                if self.error.is_none() {
                    self.error = Some(line.clone());
                }
            }
            LogLineEvent::PostConnectError => {
                if self.post_connect_error.is_none() {
                    log::warn!("[detect] post-connect error: {line}");
                    self.post_connect_error = Some(line.clone());
                }
            }
            LogLineEvent::Normal => {}
        }

        self.lines.push(line);
        if self.lines.len() > MAX_LOG_LINES {
            self.lines.remove(0);
        }
    }
}
