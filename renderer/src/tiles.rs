use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileKey {
    pub level: u32,
    pub tx: i32,
    pub tz: i32,
}

impl TileKey {
    pub fn new(level: u32, tx: i32, tz: i32) -> Self {
        Self { level, tx, tz }
    }
}

#[derive(Debug, Clone)]
pub struct TileStore {
    root: PathBuf,
}

impl TileStore {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn tile_path(&self, dimension: &str, key: TileKey) -> PathBuf {
        self.root
            .join("tiles")
            .join(dimension)
            .join(key.level.to_string())
            .join(key.tx.to_string())
            .join(format!("{}.png", key.tz))
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.json")
    }

    pub fn markers_path(&self) -> PathBuf {
        self.root.join("markers.json")
    }

    pub fn report_path(&self) -> PathBuf {
        self.root.join("report.json")
    }
}
