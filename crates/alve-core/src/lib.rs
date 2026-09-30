pub mod agents;
pub mod api;
pub mod crypto;
pub mod quality;
pub mod search;
pub mod validation;
pub mod vault;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Error {
    pub status: u16,
    pub message: String,
}
impl Error {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::new(500, "Local database operation failed.")
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::new(500, "Local file operation failed.")
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self::new(400, "Invalid JSON data.")
    }
}
pub type Result<T> = std::result::Result<T, Error>;
