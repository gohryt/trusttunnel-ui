#[derive(Clone)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Error(String),
}

impl ConnectionState {
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
