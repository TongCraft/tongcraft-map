use std::collections::BTreeMap;
use std::sync::Arc;

use fastnbt::Value;
use serde::Serialize;

use crate::config::Project;
use crate::error::Result;
use crate::reader::DimensionReader;
use crate::style::Style;

#[derive(Debug, Serialize)]
pub struct InspectReport {
    pub schema: u32,
    pub snapshot: String,
    pub output_dir: String,
    pub style_version: String,
    pub threads: usize,
    pub hardware: HardwareReport,
    pub dimensions: Vec<DimensionInspect>,
}

#[derive(Debug, Serialize)]
pub struct HardwareReport {
    pub os: String,
    pub arch: String,
    pub logical_cpus: usize,
    pub gpu: String,
    pub gpu_note: String,
}

#[derive(Debug, Serialize)]
pub struct DimensionInspect {
    pub id: String,
    pub layout: String,
    pub region_dir: String,
    pub region_files: usize,
    pub chunks: usize,
    pub bounds: Option<[i32; 4]>,
    pub status: BTreeMap<String, usize>,
    pub data_versions: BTreeMap<i32, usize>,
    pub compression: BTreeMap<String, usize>,
    pub external_chunks: usize,
    pub total_nbt_bytes: u64,
}

pub fn inspect(project: &Project) -> Result<InspectReport> {
    let style = Arc::new(Style::load(&project.style_path)?);
    let mut dimensions = Vec::with_capacity(project.dimensions.len());
    for spec in &project.dimensions {
        let reader = DimensionReader::new(spec.clone(), style.clone())?;
        let mut status = BTreeMap::new();
        let mut data_versions = BTreeMap::new();
        let mut compression = BTreeMap::new();
        let mut external_chunks = 0;
        let mut total_nbt_bytes = 0_u64;

        for (chunk_x, chunk_z) in reader.index.chunks.keys() {
            let raw = reader
                .read_raw(*chunk_x, *chunk_z)?
                .expect("indexed chunk must be readable");
            total_nbt_bytes += raw.nbt_bytes as u64;
            if raw.external {
                external_chunks += 1;
            }
            *compression
                .entry(format!(
                    "{} ({})",
                    compression_name(raw.compression),
                    raw.compression
                ))
                .or_insert(0) += 1;
            let status_name = match raw.nbt.get("Status") {
                Some(Value::String(value)) => value.clone(),
                Some(_) => "invalid".to_owned(),
                None => "missing".to_owned(),
            };
            *status.entry(status_name).or_insert(0) += 1;
            if let Some(version) = raw.nbt.get("DataVersion").and_then(Value::as_i64) {
                *data_versions.entry(version as i32).or_insert(0) += 1;
            }
        }

        dimensions.push(DimensionInspect {
            id: spec.id.clone(),
            layout: spec.layout.as_str().to_owned(),
            region_dir: relative_to(&project.snapshot, &spec.region_dir),
            region_files: reader.index.region_files,
            chunks: reader.index.chunk_count(),
            bounds: reader.index.bounds(),
            status,
            data_versions,
            compression,
            external_chunks,
            total_nbt_bytes,
        });
    }

    Ok(InspectReport {
        schema: 1,
        snapshot: project.snapshot.display().to_string(),
        output_dir: project.output_dir.display().to_string(),
        style_version: style.version.clone(),
        threads: project.threads(),
        hardware: HardwareReport {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            logical_cpus: std::thread::available_parallelism().map_or(1, |n| n.get()),
            gpu: "not_probed".to_owned(),
            gpu_note: "尚未实现 GPU 探测；阶段 C 前不声称硬件加速可用".to_owned(),
        },
        dimensions,
    })
}

fn relative_to(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn compression_name(value: u8) -> &'static str {
    match value {
        1 => "gzip",
        2 => "zlib",
        3 => "none",
        4 => "lz4",
        _ => "unknown",
    }
}
