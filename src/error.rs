use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Notion API: {0}")]
    Notion(String),
    #[error("Discord API: {0}")]
    Discord(String),
    #[error("Config: {0}")]
    Config(String),
    #[error("Database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization: {0}")]
    Ser(String),
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self { AppError::Ser(e.to_string()) }
}
impl From<toml::de::Error> for AppError {
    fn from(e: toml::de::Error) -> Self { AppError::Config(e.to_string()) }
}
impl From<toml::ser::Error> for AppError {
    fn from(e: toml::ser::Error) -> Self { AppError::Config(e.to_string()) }
}

pub type Result<T> = std::result::Result<T, AppError>;
