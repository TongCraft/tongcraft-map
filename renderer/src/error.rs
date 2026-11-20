use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("配置错误：{0}")]
    Config(String),

    #[error("快照错误：{0}")]
    Snapshot(String),

    #[error("读取区块失败（{dimension} {x},{z}）：{message}")]
    Chunk {
        dimension: String,
        x: i32,
        z: i32,
        message: String,
    },

    #[error("渲染失败（{dimension}）：{message}")]
    Render { dimension: String, message: String },

    #[error("验收失败：{0}")]
    Verify(String),

    #[error("无法读取 {path}：{source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("无法解析 {path}：{source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("无法解析配置 {path}：{source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("PNG 错误（{path}）：{message}")]
    Png { path: PathBuf, message: String },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }

    pub fn png(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Png {
            path: path.into(),
            message: message.into(),
        }
    }
}

pub fn chunk_error(dimension: &str, x: i32, z: i32, message: impl Into<String>) -> Error {
    Error::Chunk {
        dimension: dimension.to_owned(),
        x,
        z,
        message: message.into(),
    }
}
