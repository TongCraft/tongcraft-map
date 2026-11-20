use std::sync::Arc;

use crate::chunk::{extract_columns, ChunkColumns};
use crate::config::{DimensionSpec, Layer};
use crate::error::Result;
use crate::mca::{ChunkRaw, RegionIndex};
use crate::style::Style;

pub struct DimensionReader {
    pub spec: DimensionSpec,
    pub index: RegionIndex,
    pub style: Arc<Style>,
}

impl DimensionReader {
    pub fn new(spec: DimensionSpec, style: Arc<Style>) -> Result<Self> {
        let index = RegionIndex::scan(&spec.region_dir, &spec.id)?;
        Ok(Self { spec, index, style })
    }

    pub fn max_y(&self) -> i32 {
        match self.spec.layer {
            Layer::Surface => 2047,
            Layer::BelowY(y) => y,
        }
    }

    pub fn read_raw(&self, chunk_x: i32, chunk_z: i32) -> Result<Option<ChunkRaw>> {
        self.index.read_chunk(&self.spec.id, chunk_x, chunk_z)
    }

    pub fn read_columns(&self, chunk_x: i32, chunk_z: i32) -> Result<ChunkColumns> {
        let Some(raw) = self.read_raw(chunk_x, chunk_z)? else {
            return Ok(ChunkColumns::missing());
        };
        extract_columns(
            &self.spec.id,
            chunk_x,
            chunk_z,
            &raw.nbt,
            &self.style,
            self.max_y(),
        )
    }
}
