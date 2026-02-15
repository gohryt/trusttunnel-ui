use crate::theme::{COLOR_GREEN, COLOR_RED, COLOR_YELLOW, TEXT_DIM};

#[derive(Clone)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Error(String),
}

impl ConnectionState {
    pub fn label(&self) -> String {
        match self {
            Self::Disconnected => "Disconnected".into(),
            Self::Connecting => "Connecting…".into(),
            Self::Connected => "Connected".into(),
            Self::Disconnecting => "Disconnecting…".into(),
            Self::Error(message) => format!("Error: {message}"),
        }
    }

    pub fn color(&self) -> u32 {
        match self {
            Self::Disconnected => TEXT_DIM,
            Self::Connecting => COLOR_YELLOW,
            Self::Connected => COLOR_GREEN,
            Self::Disconnecting => COLOR_YELLOW,
            Self::Error(_) => COLOR_RED,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Connecting | Self::Disconnecting)
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Connected | Self::Disconnecting
        )
    }
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
}
