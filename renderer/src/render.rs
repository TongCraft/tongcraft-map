use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use serde::Serialize;

use crate::config::Project;
use crate::coverage::Coverage;
use crate::error::{Error, Result};
use crate::manifest::{DimensionManifest, Manifest};
use crate::markers::load_markers;
use crate::pyramid::build_pyramid;
use crate::raster::render_level0_tile;
use crate::reader::DimensionReader;
use crate::style::Style;
use crate::tiles::{TileKey, TileStore};
use crate::util::{floor_div, valid_release_id, write_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cpu,
    Gpu,
    Auto,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RenderSummary {
    pub release: String,
    pub staging_dir: String,
    pub dimension_count: usize,
    pub tiles: usize,
    pub tile_bytes: u64,
    pub duration_ms: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Report {
    schema: u32,
    release: String,
    requested_backend: String,
    backend: String,
    fallback_reason: Option<String>,
    style_version: String,
    threads: usize,
    generated_at_unix: u64,
    duration_ms: u64,
    peak_rss_bytes: Option<u64>,
    peak_rss_note: String,
    dimensions: Vec<DimensionReport>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DimensionReport {
    id: String,
    layout: String,
    region_dir: String,
    chunks_in_index: usize,
    chunk_reads: usize,
    chunks_sampled: usize,
    tiles: usize,
    tiles_rendered: usize,
    tile_bytes: u64,
    pyramid_bytes: u64,
    max_level: u32,
    unknown_blocks: BTreeMap<String, u64>,
}

pub fn render(
    project: &Project,
    release: &str,
    backend: Backend,
    rebuild: bool,
) -> Result<RenderSummary> {
    if !valid_release_id(release) {
        return Err(Error::Config(format!(
            "发布 ID 只能包含字母、数字、点、下划线和连字符，且长度不超过 64：{release}"
        )));
    }
    if backend == Backend::Gpu {
        return Err(Error::Render {
            dimension: "all".to_owned(),
            message: "GPU 后端尚未实现。阶段 C 需要真实硬件、真实快照和逐像素 CPU 对照；请先使用 --backend cpu".to_owned(),
        });
    }

    let started = Instant::now();
    let mut warnings = Vec::new();
    if rebuild {
        warnings.push("尚未实现增量缓存，--rebuild 不影响本次全量渲染".to_owned());
    }
    if backend == Backend::Auto {
        warnings.push("GPU 后端尚未实现，auto 已回退到 CPU".to_owned());
    }

    let staging_dir = project.output_dir.join("staging").join(release);
    if staging_dir.exists() {
        return Err(Error::Config(format!(
            "暂存版本已存在，不重用发布 ID：{}",
            staging_dir.display()
        )));
    }
    std::fs::create_dir_all(&staging_dir).map_err(|source| Error::io(&staging_dir, source))?;
    let store = TileStore::new(&staging_dir);

    let style = Arc::new(Style::load(&project.style_path)?);
    let threads = project.threads();
    let pool = ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| Error::Render {
            dimension: "all".to_owned(),
            message: format!("无法创建线程池：{error}"),
        })?;

    let dimension_ids: HashSet<String> = project
        .dimensions
        .iter()
        .map(|dimension| dimension.id.clone())
        .collect();
    let markers = load_markers(project.markers_path.as_deref(), &dimension_ids)?;
    warnings.extend(markers.warnings);

    let mut manifest_dimensions = Vec::with_capacity(project.dimensions.len());
    let mut reports = Vec::with_capacity(project.dimensions.len());
    let mut total_tiles = 0;
    let mut total_bytes = 0_u64;

    for spec in &project.dimensions {
        let reader = DimensionReader::new(spec.clone(), style.clone())?;
        let fallback_bounds = spec
            .bounds
            .or_else(|| reader.index.bounds())
            .unwrap_or([0, 0, 1, 1]);
        if reader.index.chunks.is_empty() {
            warnings.push(format!("维度 {} 没有找到区块；该维度将没有瓦片", spec.id));
        }
        let tile_keys = level0_tiles(fallback_bounds);
        let dimension_started = Instant::now();

        let results = pool.install(|| {
            tile_keys
                .par_iter()
                .map(|key| render_level0_tile(&reader, fallback_bounds, *key, &store))
                .collect::<Result<Vec<_>>>()
        })?;

        let mut coverage = Coverage::default();
        let mut chunk_reads = 0;
        let mut sampled_chunks = HashSet::new();
        let mut unknown_seen = HashSet::new();
        let mut unknown: BTreeMap<String, u64> = BTreeMap::new();
        let mut tile_bytes = 0_u64;
        let mut tiles_rendered = 0;
        let mut rendered_content: Option<[i32; 4]> = None;

        for result in &results {
            chunk_reads += result.chunks_sampled;
            if result.covered {
                coverage.insert(0, result.key.tx, result.key.tz);
                tile_bytes += result.bytes;
                tiles_rendered += 1;
                if let Some(bounds) = result.content_bounds {
                    rendered_content = Some(match rendered_content {
                        None => bounds,
                        Some(current) => [
                            current[0].min(bounds[0]),
                            current[1].min(bounds[1]),
                            current[2].max(bounds[2]),
                            current[3].max(bounds[3]),
                        ],
                    });
                }
            }
            for chunk in &result.sampled_chunks {
                sampled_chunks.insert(*chunk);
            }
            for ((chunk_x, chunk_z), names) in &result.chunk_unknowns {
                if !unknown_seen.insert((*chunk_x, *chunk_z)) {
                    continue;
                }
                for (name, count) in names {
                    *unknown.entry(name.clone()).or_insert(0) += count;
                }
            }
        }

        let content_bounds = rendered_content.unwrap_or(fallback_bounds);
        let extent = (content_bounds[2] - content_bounds[0])
            .max(content_bounds[3] - content_bounds[1])
            .max(1) as i64;
        let pyramid = build_pyramid(
            &spec.id,
            &store,
            coverage,
            project.effective_max_level(spec),
            extent,
        )?;

        let dimension_elapsed = dimension_started.elapsed();
        let coverage_rel = format!("coverage/{}.json", spec.id);
        pyramid
            .coverage
            .write(&staging_dir.join(&coverage_rel), &spec.id)?;

        let bounds = rendered_content.unwrap_or(fallback_bounds);
        manifest_dimensions.push(DimensionManifest {
            id: spec.id.clone(),
            bounds,
            max_level: pyramid.coverage.max_level(),
            coverage: coverage_rel,
            tile_template: format!("tiles/{}/{{level}}/{{tx}}/{{tz}}.png", spec.id),
        });

        reports.push(DimensionReport {
            id: spec.id.clone(),
            layout: spec.layout.as_str().to_owned(),
            region_dir: relative_to(&project.snapshot, &spec.region_dir),
            chunks_in_index: reader.index.chunk_count(),
            chunk_reads,
            chunks_sampled: sampled_chunks.len(),
            tiles: pyramid.tiles,
            tiles_rendered,
            tile_bytes,
            pyramid_bytes: pyramid.bytes,
            max_level: pyramid.coverage.max_level(),
            unknown_blocks: unknown,
        });
        total_tiles += pyramid.tiles;
        total_bytes += tile_bytes + pyramid.bytes;

        eprintln!(
            "维度 {}：{} 个瓦片，{} 个区块索引，耗时 {:.1}s",
            spec.id,
            pyramid.tiles,
            reader.index.chunk_count(),
            dimension_elapsed.as_secs_f64()
        );
    }

    let manifest = Manifest::new(release, manifest_dimensions);
    manifest.write(&store.manifest_path())?;
    write_json(&store.markers_path(), &markers.markers)?;

    let duration = started.elapsed();
    let report = Report {
        schema: 1,
        release: release.to_owned(),
        requested_backend: backend.as_str().to_owned(),
        backend: "cpu".to_owned(),
        fallback_reason: (backend == Backend::Auto)
            .then(|| "GPU 后端尚未实现，已回退到 CPU".to_owned()),
        style_version: style.version.clone(),
        threads,
        generated_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs(),
        duration_ms: duration.as_millis() as u64,
        peak_rss_bytes: None,
        peak_rss_note: "尚未采集峰值内存".to_owned(),
        dimensions: reports,
        warnings: warnings.clone(),
    };
    write_json(&store.report_path(), &report)?;

    Ok(RenderSummary {
        release: release.to_owned(),
        staging_dir: staging_dir.display().to_string(),
        dimension_count: project.dimensions.len(),
        tiles: total_tiles,
        tile_bytes: total_bytes,
        duration_ms: duration.as_millis() as u64,
        warnings,
    })
}

fn level0_tiles(bounds: [i32; 4]) -> Vec<TileKey> {
    if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
        return Vec::new();
    }
    let tx_min = floor_div(bounds[0], 256);
    let tx_max = floor_div(bounds[2] - 1, 256);
    let tz_min = floor_div(bounds[1], 256);
    let tz_max = floor_div(bounds[3] - 1, 256);
    let mut keys = Vec::new();
    for tz in tz_min..=tz_max {
        for tx in tx_min..=tx_max {
            keys.push(TileKey::new(0, tx, tz));
        }
    }
    keys
}

fn relative_to(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}
