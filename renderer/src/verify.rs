use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::coverage::Coverage;
use crate::error::{Error, Result};
use crate::manifest::Manifest;
use crate::markers::Marker;
use crate::util::{read_json, valid_dimension_id, valid_release_id, validate_release_relative};

#[derive(Debug, Serialize)]
pub struct VerifyReport {
    pub schema: u32,
    pub release: String,
    pub dimensions: Vec<DimensionVerify>,
    pub tiles: usize,
    pub sampled_tiles: usize,
    pub markers: usize,
}

#[derive(Debug, Serialize)]
pub struct DimensionVerify {
    pub id: String,
    pub bounds: [i32; 4],
    pub max_level: u32,
    pub tiles: usize,
    pub levels: BTreeMap<u32, usize>,
}

pub fn verify(release_dir: &Path) -> Result<VerifyReport> {
    let release_dir = release_dir
        .canonicalize()
        .map_err(|source| Error::io(release_dir, source))?;
    if !release_dir.is_dir() {
        return Err(Error::Verify(format!(
            "版本目录不存在：{}",
            release_dir.display()
        )));
    }
    let manifest_path = release_dir.join("manifest.json");
    let manifest = Manifest::read(&manifest_path)?;
    if manifest.schema != 1 {
        return Err(Error::Verify(format!(
            "manifest schema 不是 1：{}",
            manifest.schema
        )));
    }
    if !valid_release_id(&manifest.release) {
        return Err(Error::Verify(format!(
            "manifest release ID 无效：{}",
            manifest.release
        )));
    }
    if manifest.tile_size != 256 {
        return Err(Error::Verify(format!(
            "tileSize 必须是 256，收到 {}",
            manifest.tile_size
        )));
    }
    if manifest.format != "png" {
        return Err(Error::Verify(format!(
            "当前只验证 PNG 瓦片，收到 {}",
            manifest.format
        )));
    }
    if manifest.dimensions.is_empty() {
        return Err(Error::Verify("manifest 没有任何维度".to_owned()));
    }

    let mut dimension_ids = BTreeSet::new();
    let mut total_tiles = 0;
    let mut sampled_tiles = 0;
    let mut dimensions = Vec::with_capacity(manifest.dimensions.len());

    for dimension in &manifest.dimensions {
        if !valid_dimension_id(&dimension.id) {
            return Err(Error::Verify(format!("维度 ID 无效：{}", dimension.id)));
        }
        if !dimension_ids.insert(dimension.id.clone()) {
            return Err(Error::Verify(format!("维度 ID 重复：{}", dimension.id)));
        }
        if dimension.bounds[0] > dimension.bounds[2] || dimension.bounds[1] > dimension.bounds[3] {
            return Err(Error::Verify(format!(
                "维度 {} 的 bounds 无效：{:?}",
                dimension.id, dimension.bounds
            )));
        }
        if dimension.max_level > 12 {
            return Err(Error::Verify(format!(
                "维度 {} 的 maxLevel 异常：{}",
                dimension.id, dimension.max_level
            )));
        }
        let coverage_path = safe_release_path(&release_dir, &dimension.coverage)?;
        let (coverage_dimension, coverage) = Coverage::read(&coverage_path)?;
        if coverage_dimension != dimension.id {
            return Err(Error::Verify(format!(
                "维度 {} 的 coverage 声明维度是 {}",
                dimension.id, coverage_dimension
            )));
        }
        if coverage.tile_count() == 0 {
            if dimension.max_level != 0 {
                return Err(Error::Verify(format!(
                    "维度 {} 没有瓦片但 maxLevel 是 {}",
                    dimension.id, dimension.max_level
                )));
            }
            dimensions.push(DimensionVerify {
                id: dimension.id.clone(),
                bounds: dimension.bounds,
                max_level: 0,
                tiles: 0,
                levels: BTreeMap::new(),
            });
            continue;
        }
        if coverage.max_level() != dimension.max_level {
            return Err(Error::Verify(format!(
                "维度 {} 的 coverage 最高层 {} 与 manifest 的 maxLevel {} 不一致",
                dimension.id,
                coverage.max_level(),
                dimension.max_level
            )));
        }
        if let Some(level0_bounds) = coverage.level0_bounds() {
            let within = dimension.bounds[0] >= level0_bounds[0]
                && dimension.bounds[1] >= level0_bounds[1]
                && dimension.bounds[2] <= level0_bounds[2]
                && dimension.bounds[3] <= level0_bounds[3];
            if !within || dimension.bounds[2] <= dimension.bounds[0] {
                return Err(Error::Verify(format!(
                    "维度 {} 的 manifest bounds {:?} 不在 level 0 覆盖 {:?} 内",
                    dimension.id, dimension.bounds, level0_bounds
                )));
            }
        }
        for level in 0..=dimension.max_level {
            if coverage.level_count(level) == 0 {
                return Err(Error::Verify(format!(
                    "维度 {} 的覆盖层 {} 缺失",
                    dimension.id, level
                )));
            }
        }

        let mut level_counts = BTreeMap::new();
        let mut expected_files = BTreeSet::new();
        let mut tiles_for_level: BTreeMap<u32, Vec<(i32, i32)>> = BTreeMap::new();
        for level in 0..=dimension.max_level {
            let mut tiles: Vec<(i32, i32)> = coverage.level_tiles(level).collect();
            tiles.sort_unstable();
            level_counts.insert(level, tiles.len());
            total_tiles += tiles.len();
            tiles_for_level.insert(level, tiles);
            for (tx, tz) in coverage.level_tiles(level) {
                let path = expand_tile_path(&release_dir, &dimension.tile_template, level, tx, tz)?;
                if !path.is_file() {
                    return Err(Error::Verify(format!(
                        "维度 {} 声明了 level {level} 瓦片 ({tx},{tz})，文件缺失：{}",
                        dimension.id,
                        path.display()
                    )));
                }
                expected_files.insert(
                    path.canonicalize()
                        .map_err(|source| Error::io(&path, source))?,
                );
            }
        }

        for (level, tiles) in &tiles_for_level {
            let pick: Vec<(i32, i32)> = if tiles.len() <= 32 {
                tiles.clone()
            } else {
                let mut picked = Vec::with_capacity(32);
                picked.extend_from_slice(&tiles[..16]);
                picked.extend_from_slice(&tiles[tiles.len() - 16..]);
                picked
            };
            for (tx, tz) in pick {
                let path =
                    expand_tile_path(&release_dir, &dimension.tile_template, *level, tx, tz)?;
                decode_tile(&path)?;
                sampled_tiles += 1;
            }
        }

        let tiles_root = release_dir.join("tiles").join(&dimension.id);
        if tiles_root.is_dir() {
            for entry in WalkDir::new(&tiles_root) {
                let entry = entry.map_err(|error| {
                    Error::Verify(format!("遍历 {} 失败：{error}", tiles_root.display()))
                })?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry
                    .path()
                    .canonicalize()
                    .map_err(|source| Error::io(entry.path(), source))?;
                if !expected_files.contains(&path) {
                    return Err(Error::Verify(format!(
                        "维度 {} 存在覆盖索引之外的瓦片：{}",
                        dimension.id,
                        path.display()
                    )));
                }
            }
        }

        dimensions.push(DimensionVerify {
            id: dimension.id.clone(),
            bounds: dimension.bounds,
            max_level: dimension.max_level,
            tiles: level_counts.values().sum(),
            levels: level_counts,
        });
    }

    let markers_path = safe_release_path(&release_dir, &manifest.markers)?;
    let markers: Vec<Marker> = read_json(&markers_path)?;
    let mut marker_ids = BTreeSet::new();
    for marker in &markers {
        if marker.id.is_empty() || marker.name.is_empty() {
            return Err(Error::Verify("标记 id/name 不能为空".to_owned()));
        }
        if !marker_ids.insert(marker.id.clone()) {
            return Err(Error::Verify(format!("标记 ID 重复：{}", marker.id)));
        }
        if !dimension_ids.contains(&marker.dimension) {
            return Err(Error::Verify(format!(
                "标记 {} 指向不存在的维度 {}",
                marker.id, marker.dimension
            )));
        }
        if !marker.x.is_finite() || !marker.z.is_finite() {
            return Err(Error::Verify(format!(
                "标记 {} 的 x/z 不是有限数字",
                marker.id
            )));
        }
        if marker.y.is_some_and(|y| !y.is_finite()) {
            return Err(Error::Verify(format!(
                "标记 {} 的 y 不是有限数字",
                marker.id
            )));
        }
    }

    Ok(VerifyReport {
        schema: 1,
        release: manifest.release,
        dimensions,
        tiles: total_tiles,
        sampled_tiles,
        markers: markers.len(),
    })
}

fn safe_release_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    validate_release_relative(relative)?;
    let path = root.join(relative);
    let canonical = path
        .canonicalize()
        .map_err(|source| Error::io(&path, source))?;
    if !canonical.starts_with(root) {
        return Err(Error::Verify(format!(
            "路径越出版本目录：{}",
            relative.display()
        )));
    }
    Ok(canonical)
}

fn expand_tile_path(root: &Path, template: &str, level: u32, tx: i32, tz: i32) -> Result<PathBuf> {
    for placeholder in ["{level}", "{tx}", "{tz}"] {
        if !template.contains(placeholder) {
            return Err(Error::Verify(format!(
                "tileTemplate 缺少 {placeholder}：{template}"
            )));
        }
    }
    let relative = template
        .replace("{level}", &level.to_string())
        .replace("{tx}", &tx.to_string())
        .replace("{tz}", &tz.to_string());
    let relative = Path::new(&relative);
    validate_release_relative(relative)?;
    Ok(root.join(relative))
}

fn decode_tile(path: &Path) -> Result<()> {
    let file = std::fs::File::open(path).map_err(|source| Error::io(path, source))?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder
        .read_info()
        .map_err(|error| Error::png(path, error.to_string()))?;
    if reader.info().width != 256 || reader.info().height != 256 {
        return Err(Error::png(
            path,
            format!(
                "瓦片尺寸 {}×{}，期望 256×256",
                reader.info().width,
                reader.info().height
            ),
        ));
    }
    let mut buffer = vec![0_u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| Error::png(path, error.to_string()))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(Error::png(
            path,
            format!(
                "瓦片格式 {:?}/{:?}，期望 RGBA8",
                info.color_type, info.bit_depth
            ),
        ));
    }
    Ok(())
}
