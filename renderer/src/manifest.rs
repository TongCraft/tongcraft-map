use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::util::{read_json, write_json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub release: String,
    pub tile_size: u32,
    pub format: String,
    pub markers: String,
    pub dimensions: Vec<DimensionManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DimensionManifest {
    pub id: String,
    pub bounds: [i32; 4],
    pub max_level: u32,
    pub coverage: String,
    pub tile_template: String,
}

impl Manifest {
    pub fn new(release: &str, dimensions: Vec<DimensionManifest>) -> Self {
        Self {
            schema: 1,
            release: release.to_owned(),
            tile_size: 256,
            format: "png".to_owned(),
            markers: "markers.json".to_owned(),
            dimensions,
        }
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        write_json(path, self)
    }

    pub fn read(path: &Path) -> Result<Self> {
        read_json(path)
    }
}
