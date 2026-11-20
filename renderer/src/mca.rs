use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use fastnbt::Value;
use flate2::read::{GzDecoder, ZlibDecoder};

use crate::error::{chunk_error, Error, Result};

const HEADER_BYTES: usize = 8192;
const SECTOR_BYTES: usize = 4096;
const MAX_COMPRESSED_CHUNK: u32 = 64 * 1024 * 1024;
const MAX_NBT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ChunkLocation {
    pub path: PathBuf,
    pub sector_offset: u32,
    pub sectors: u8,
}

#[derive(Debug)]
pub struct ChunkRaw {
    pub nbt: BTreeMap<String, Value>,
    pub compression: u8,
    pub external: bool,
    pub nbt_bytes: usize,
}

#[derive(Debug)]
pub struct RegionIndex {
    pub chunks: BTreeMap<(i32, i32), ChunkLocation>,
    pub region_files: usize,
}

impl RegionIndex {
    pub fn scan(region_dir: &Path, dimension: &str) -> Result<Self> {
        let entries =
            std::fs::read_dir(region_dir).map_err(|source| Error::io(region_dir, source))?;
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| Error::io(region_dir, source))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with("r.") || !name.ends_with(".mca") {
                continue;
            }
            let coords = parse_region_name(name).ok_or_else(|| {
                Error::Snapshot(format!(
                    "维度 {dimension} 的 region 文件名无法解析：{}",
                    path.display()
                ))
            })?;
            files.push((coords, path));
        }
        files.sort_by_key(|(coords, _)| *coords);

        let mut chunks = BTreeMap::new();
        for ((region_x, region_z), path) in &files {
            let mut file = File::open(path).map_err(|source| Error::io(path, source))?;
            let mut header = [0_u8; HEADER_BYTES];
            file.read_exact(&mut header)
                .map_err(|source| Error::io(path, source))?;
            for local_z in 0..32_i32 {
                for local_x in 0..32_i32 {
                    let index = (local_x + local_z * 32) as usize;
                    let location = u32::from_be_bytes([
                        header[index * 4],
                        header[index * 4 + 1],
                        header[index * 4 + 2],
                        header[index * 4 + 3],
                    ]);
                    let sector_offset = location >> 8;
                    let sectors = (location & 0xFF) as u8;
                    if sector_offset == 0 {
                        continue;
                    }
                    if sectors == 0 {
                        return Err(Error::Snapshot(format!(
                            "维度 {dimension} 的 region {} 中区块 ({},{}) 的长度为 0",
                            path.display(),
                            region_x * 32 + local_x,
                            region_z * 32 + local_z
                        )));
                    }
                    let chunk_x = region_x * 32 + local_x;
                    let chunk_z = region_z * 32 + local_z;
                    if chunks
                        .insert(
                            (chunk_x, chunk_z),
                            ChunkLocation {
                                path: path.clone(),
                                sector_offset,
                                sectors,
                            },
                        )
                        .is_some()
                    {
                        return Err(Error::Snapshot(format!(
                            "维度 {dimension} 的区块 ({chunk_x},{chunk_z}) 在多个 region 中重复出现"
                        )));
                    }
                }
            }
        }

        Ok(Self {
            chunks,
            region_files: files.len(),
        })
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn bounds(&self) -> Option<[i32; 4]> {
        let min_x = self.chunks.keys().map(|(x, _)| *x).min()?;
        let max_x = self.chunks.keys().map(|(x, _)| *x).max()?;
        let min_z = self.chunks.keys().map(|(_, z)| *z).min()?;
        let max_z = self.chunks.keys().map(|(_, z)| *z).max()?;
        Some([min_x * 16, min_z * 16, (max_x + 1) * 16, (max_z + 1) * 16])
    }

    pub fn read_chunk(
        &self,
        dimension: &str,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Result<Option<ChunkRaw>> {
        let Some(location) = self.chunks.get(&(chunk_x, chunk_z)) else {
            return Ok(None);
        };
        read_chunk_raw(dimension, chunk_x, chunk_z, location).map(Some)
    }
}

fn parse_region_name(name: &str) -> Option<(i32, i32)> {
    let stem = name.strip_prefix("r.")?.strip_suffix(".mca")?;
    let (x, z) = stem.split_once('.')?;
    if z.contains('.') {
        return None;
    }
    Some((x.parse().ok()?, z.parse().ok()?))
}

fn read_chunk_raw(
    dimension: &str,
    chunk_x: i32,
    chunk_z: i32,
    location: &ChunkLocation,
) -> Result<ChunkRaw> {
    let path = &location.path;
    let mut file = File::open(path).map_err(|source| Error::io(path, source))?;
    let start = location.sector_offset as u64 * SECTOR_BYTES as u64;
    file.seek(SeekFrom::Start(start))
        .map_err(|source| Error::io(path, source))?;

    let mut length_bytes = [0_u8; 4];
    file.read_exact(&mut length_bytes).map_err(|source| {
        chunk_error(
            dimension,
            chunk_x,
            chunk_z,
            format!("读取区块长度失败（{}）：{source}", path.display()),
        )
    })?;
    let length = u32::from_be_bytes(length_bytes);
    let available = location.sectors as u32 * SECTOR_BYTES as u32;
    if length < 1 || length + 4 > available {
        return Err(chunk_error(
            dimension,
            chunk_x,
            chunk_z,
            format!("区块长度 {length} 超出已分配扇区（{} 字节）", available),
        ));
    }
    if length > MAX_COMPRESSED_CHUNK {
        return Err(chunk_error(
            dimension,
            chunk_x,
            chunk_z,
            format!("压缩区块过大：{length} 字节"),
        ));
    }
    let mut payload = vec![0_u8; length as usize];
    file.read_exact(&mut payload).map_err(|source| {
        chunk_error(
            dimension,
            chunk_x,
            chunk_z,
            format!("读取区块数据失败（{}）：{source}", path.display()),
        )
    })?;

    let compression_byte = payload[0];
    let external = compression_byte & 0x80 != 0;
    let compression = compression_byte & 0x7F;
    let compressed: Vec<u8> = if external {
        let external_path = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("c.{chunk_x}.{chunk_z}.mcc"));
        read_limited(&external_path, MAX_NBT_BYTES).map_err(|source| {
            chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!(
                    "外置区块数据缺失或无法读取（{}）：{source}",
                    external_path.display()
                ),
            )
        })?
    } else {
        payload[1..].to_vec()
    };

    let nbt_bytes = decompress(dimension, chunk_x, chunk_z, compression, &compressed)?;
    let nbt: BTreeMap<String, Value> = fastnbt::from_bytes(&nbt_bytes).map_err(|source| {
        chunk_error(
            dimension,
            chunk_x,
            chunk_z,
            format!("NBT 解析失败：{source}"),
        )
    })?;

    let nbt_x = nbt
        .get("xPos")
        .and_then(Value::as_i64)
        .ok_or_else(|| chunk_error(dimension, chunk_x, chunk_z, "缺少 xPos"))?;
    let nbt_z = nbt
        .get("zPos")
        .and_then(Value::as_i64)
        .ok_or_else(|| chunk_error(dimension, chunk_x, chunk_z, "缺少 zPos"))?;
    if nbt_x != chunk_x as i64 || nbt_z != chunk_z as i64 {
        return Err(chunk_error(
            dimension,
            chunk_x,
            chunk_z,
            format!("NBT 坐标 ({nbt_x},{nbt_z}) 与 region 索引不一致"),
        ));
    }

    Ok(ChunkRaw {
        nbt,
        compression,
        external,
        nbt_bytes: nbt_bytes.len(),
    })
}

fn decompress(
    dimension: &str,
    x: i32,
    z: i32,
    compression: u8,
    compressed: &[u8],
) -> Result<Vec<u8>> {
    match compression {
        1 => read_decoder(dimension, x, z, "gzip", GzDecoder::new(compressed)),
        2 => read_decoder(dimension, x, z, "zlib", ZlibDecoder::new(compressed)),
        3 => {
            if compressed.len() > MAX_NBT_BYTES {
                Err(chunk_error(
                    dimension,
                    x,
                    z,
                    format!("未压缩 NBT 过大：{} 字节", compressed.len()),
                ))
            } else {
                Ok(compressed.to_vec())
            }
        }
        4 => Err(chunk_error(
            dimension,
            x,
            z,
            "暂不支持 LZ4 区块压缩（compression=4）",
        )),
        other => Err(chunk_error(
            dimension,
            x,
            z,
            format!("未知区块压缩类型 {other}"),
        )),
    }
}

fn read_decoder<R: Read>(
    dimension: &str,
    x: i32,
    z: i32,
    name: &str,
    reader: R,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    reader
        .take(MAX_NBT_BYTES as u64 + 1)
        .read_to_end(&mut output)
        .map_err(|source| chunk_error(dimension, x, z, format!("{name} 解压失败：{source}")))?;
    if output.len() > MAX_NBT_BYTES {
        return Err(chunk_error(
            dimension,
            x,
            z,
            format!("解压后 NBT 超过 {MAX_NBT_BYTES} 字节"),
        ));
    }
    Ok(output)
}

fn read_limited(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("文件超过 {limit} 字节限制"),
        ));
    }
    Ok(bytes)
}
