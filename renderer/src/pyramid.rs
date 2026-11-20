use std::collections::BTreeSet;
use std::path::Path;

use crate::coverage::Coverage;
use crate::error::{Error, Result};
use crate::raster::encode_rgba;
use crate::tiles::{TileKey, TileStore};
use crate::util::write_bytes;

const TILE: u32 = 256;

pub struct PyramidResult {
    pub coverage: Coverage,
    pub tiles: usize,
    pub bytes: u64,
}

pub fn build_pyramid(
    dimension: &str,
    store: &TileStore,
    level0: Coverage,
    max_level: u32,
    extent: i64,
) -> Result<PyramidResult> {
    let mut coverage = level0;
    let mut tiles = coverage.level_count(0);
    let mut bytes = 0_u64;
    let mut level = 0_u32;
    if (TILE as i64) << level >= extent {
        return Ok(PyramidResult {
            coverage,
            tiles,
            bytes,
        });
    }
    while level < max_level {
        let parent_tiles: BTreeSet<(i32, i32)> = coverage
            .level_tiles(level)
            .map(|(tx, tz)| (tx.div_euclid(2), tz.div_euclid(2)))
            .collect();
        if parent_tiles.is_empty() {
            break;
        }
        let next_level = level + 1;
        let mut parent_rendered = 0_usize;
        for (tx, tz) in &parent_tiles {
            let key = TileKey::new(next_level, *tx, *tz);
            let path = store.tile_path(dimension, key);
            let mut children = [None, None, None, None];
            for (index, (dx, dz)) in [(0, 0), (1, 0), (0, 1), (1, 1)].iter().enumerate() {
                let child = TileKey::new(level, tx * 2 + dx, tz * 2 + dz);
                let child_path = store.tile_path(dimension, child);
                if child_path.is_file() {
                    children[index] = Some(decode_rgba(&child_path)?);
                }
            }
            let (pixels, covered) = downsample(&children);
            if !covered {
                continue;
            }
            let encoded = encode_rgba(TILE, TILE, &pixels).map_err(|message| Error::Png {
                path: path.clone(),
                message,
            })?;
            write_bytes(&path, &encoded)?;
            coverage.insert(next_level, *tx, *tz);
            parent_rendered += 1;
            tiles += 1;
            bytes += encoded.len() as u64;
        }
        if parent_rendered == 0 {
            break;
        }
        level = next_level;
        if parent_rendered == 1 || (TILE as i64) << level >= extent {
            break;
        }
    }

    Ok(PyramidResult {
        coverage,
        tiles,
        bytes,
    })
}

fn decode_rgba(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|source| Error::io(path, source))?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder
        .read_info()
        .map_err(|error| Error::png(path, error.to_string()))?;
    if reader.info().width != TILE || reader.info().height != TILE {
        return Err(Error::png(
            path,
            format!(
                "瓦片尺寸 {}×{}，期望 {TILE}×{TILE}",
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
    buffer.truncate(info.buffer_size());
    if buffer.len() != (TILE * TILE * 4) as usize {
        return Err(Error::png(
            path,
            format!("解码后长度 {} 不符合 RGBA8 瓦片", buffer.len()),
        ));
    }
    Ok(buffer)
}

fn downsample(children: &[Option<Vec<u8>>; 4]) -> (Vec<u8>, bool) {
    let mut output = vec![0_u8; (TILE * TILE * 4) as usize];
    let mut covered = false;
    const HALF: u32 = TILE / 2;
    for (child_index, child) in children.iter().enumerate() {
        let Some(child) = child else {
            continue;
        };
        let quadrant_x = (child_index % 2) as u32 * HALF;
        let quadrant_z = (child_index / 2) as u32 * HALF;
        for pixel_z in 0..HALF {
            for pixel_x in 0..HALF {
                let mut alpha_sum = 0_u32;
                let mut red_sum = 0_u32;
                let mut green_sum = 0_u32;
                let mut blue_sum = 0_u32;
                for (dy, dx) in [(0_u32, 0_u32), (0, 1), (1, 0), (1, 1)] {
                    let source_x = pixel_x * 2 + dx;
                    let source_z = pixel_z * 2 + dy;
                    let offset = ((source_z * TILE + source_x) * 4) as usize;
                    let alpha = child[offset + 3] as u32;
                    if alpha == 0 {
                        continue;
                    }
                    alpha_sum += alpha;
                    red_sum += child[offset] as u32 * alpha;
                    green_sum += child[offset + 1] as u32 * alpha;
                    blue_sum += child[offset + 2] as u32 * alpha;
                }
                if alpha_sum == 0 {
                    continue;
                }
                covered = true;
                let target_x = quadrant_x + pixel_x;
                let target_z = quadrant_z + pixel_z;
                let offset = ((target_z * TILE + target_x) * 4) as usize;
                output[offset] = ((red_sum + alpha_sum / 2) / alpha_sum) as u8;
                output[offset + 1] = ((green_sum + alpha_sum / 2) / alpha_sum) as u8;
                output[offset + 2] = ((blue_sum + alpha_sum / 2) / alpha_sum) as u8;
                output[offset + 3] = ((alpha_sum + 2) / 4) as u8;
            }
        }
    }
    (output, covered)
}
