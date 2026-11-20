use std::collections::BTreeMap;

use crate::columns::Column;
use crate::error::Result;
use crate::reader::DimensionReader;
use crate::tiles::{TileKey, TileStore};
use crate::util::{floor_div, write_bytes};

const TILE: i32 = 256;
const WIDTH: i32 = TILE + 2;

#[derive(Debug)]
pub struct TileOutput {
    pub key: TileKey,
    pub covered: bool,
    pub bytes: u64,
    pub content_bounds: Option<[i32; 4]>,
    pub chunks_sampled: usize,
    pub sampled_chunks: Vec<(i32, i32)>,
    pub chunk_unknowns: Vec<((i32, i32), BTreeMap<String, u64>)>,
}

pub fn render_level0_tile(
    reader: &DimensionReader,
    bounds: [i32; 4],
    key: TileKey,
    store: &TileStore,
) -> Result<TileOutput> {
    debug_assert_eq!(key.level, 0);
    let origin_x = key.tx * TILE;
    let origin_z = key.tz * TILE;
    let start_x = origin_x - 1;
    let start_z = origin_z - 1;
    let end_x = origin_x + TILE;
    let end_z = origin_z + TILE;

    let mut columns = vec![Column::missing(); (WIDTH * WIDTH) as usize];
    let mut chunk_unknowns = Vec::new();
    let mut sampled_chunks = Vec::new();
    let mut chunks_sampled = 0;

    let chunk_x0 = floor_div(start_x, 16);
    let chunk_x1 = floor_div(end_x, 16);
    let chunk_z0 = floor_div(start_z, 16);
    let chunk_z1 = floor_div(end_z, 16);

    for chunk_z in chunk_z0..=chunk_z1 {
        for chunk_x in chunk_x0..=chunk_x1 {
            let chunk = reader.read_columns(chunk_x, chunk_z)?;
            if !matches!(chunk.state, crate::chunk::ChunkState::Full) {
                continue;
            }
            chunks_sampled += 1;
            sampled_chunks.push((chunk_x, chunk_z));
            if !chunk.unknown.is_empty() {
                chunk_unknowns.push(((chunk_x, chunk_z), chunk.unknown));
            }
            for local_z in 0..16 {
                let world_z = chunk_z * 16 + local_z;
                if world_z < bounds[1] || world_z >= bounds[3] {
                    continue;
                }
                let target_z = world_z - start_z;
                if !(0..WIDTH).contains(&target_z) {
                    continue;
                }
                for local_x in 0..16 {
                    let world_x = chunk_x * 16 + local_x;
                    if world_x < bounds[0] || world_x >= bounds[2] {
                        continue;
                    }
                    let target_x = world_x - start_x;
                    if !(0..WIDTH).contains(&target_x) {
                        continue;
                    }
                    let source = (local_z * 16 + local_x) as usize;
                    let target = (target_z * WIDTH + target_x) as usize;
                    columns[target] = chunk.columns[source];
                }
            }
        }
    }

    let mut pixels = vec![0_u8; (TILE * TILE * 4) as usize];
    let mut covered = false;
    let mut content: Option<[i32; 4]> = None;
    for pixel_z in 0..TILE {
        for pixel_x in 0..TILE {
            let index = ((pixel_z + 1) * WIDTH + (pixel_x + 1)) as usize;
            let column = columns[index];
            if !column.is_valid() {
                continue;
            }
            covered = true;
            let world_x = origin_x + pixel_x;
            let world_z = origin_z + pixel_z;
            content = Some(match content {
                None => [world_x, world_z, world_x + 1, world_z + 1],
                Some([min_x, min_z, max_x, max_z]) => [
                    min_x.min(world_x),
                    min_z.min(world_z),
                    max_x.max(world_x + 1),
                    max_z.max(world_z + 1),
                ],
            });
            let (mut r, mut g, mut b) = {
                let color = reader.style.color(column.material());
                (color.r as i32, color.g as i32, color.b as i32)
            };
            if column.water_depth() > 0 && !reader.style.is_water_material(column.material()) {
                let depth = column.water_depth() as u32;
                let alpha = (reader.style.water_alpha_min as u32
                    + reader.style.water_alpha_step as u32 * depth.saturating_sub(1))
                .min(reader.style.water_alpha_max as u32)
                .min(255);
                let water = reader.style.water;
                let inverse = 255 - alpha;
                r = (water.r as u32 * alpha + r as u32 * inverse + 127) as i32 / 255;
                g = (water.g as u32 * alpha + g as u32 * inverse + 127) as i32 / 255;
                b = (water.b as u32 * alpha + b as u32 * inverse + 127) as i32 / 255;
            }

            let height = column.height();
            let mut shade = 0_i32;
            for (dx, dz) in [(-1_i32, 0_i32), (1, 0), (0, -1), (0, 1)] {
                let neighbor = columns[(index as i32 + dz * WIDTH + dx) as usize];
                let neighbor_height = if neighbor.is_valid() {
                    neighbor.height()
                } else {
                    height
                };
                shade += (neighbor_height - height).clamp(-2, 2);
            }
            let shade = (shade * 5).clamp(-30, 30);
            r = (r + shade).clamp(0, 255);
            g = (g + shade).clamp(0, 255);
            b = (b + shade).clamp(0, 255);

            let offset = ((pixel_z * TILE + pixel_x) * 4) as usize;
            pixels[offset] = r as u8;
            pixels[offset + 1] = g as u8;
            pixels[offset + 2] = b as u8;
            pixels[offset + 3] = 255;
        }
    }

    let path = store.tile_path(&reader.spec.id, key);
    if !covered {
        return Ok(TileOutput {
            key,
            covered: false,
            bytes: 0,
            content_bounds: None,
            chunks_sampled,
            sampled_chunks,
            chunk_unknowns,
        });
    }

    let bytes = encode_png(&pixels).map_err(|message| crate::error::Error::Png {
        path: path.clone(),
        message,
    })?;
    write_bytes(&path, &bytes)?;

    Ok(TileOutput {
        key,
        covered: true,
        bytes: bytes.len() as u64,
        content_bounds: content,
        chunks_sampled,
        sampled_chunks,
        chunk_unknowns,
    })
}

pub fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> std::result::Result<Vec<u8>, String> {
    if pixels.len() != (width * height * 4) as usize {
        return Err(format!(
            "RGBA 长度 {} 与 {width}×{height} 不匹配",
            pixels.len()
        ));
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(pixels)
            .map_err(|error| error.to_string())?;
    }
    Ok(output)
}

fn encode_png(pixels: &[u8]) -> std::result::Result<Vec<u8>, String> {
    encode_rgba(TILE as u32, TILE as u32, pixels)
}
