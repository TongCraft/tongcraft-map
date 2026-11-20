use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Resolve a path for comparison without requiring its leaf to exist.
pub fn canonical_for_compare(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let absolute = absolute(path);
    if let Some(name) = absolute.file_name() {
        if let Some(parent) = absolute.parent() {
            if let Ok(parent) = parent.canonicalize() {
                return parent.join(name);
            }
        }
    }
    absolute
}

pub fn is_inside(child: &Path, parent: &Path) -> bool {
    let child = canonical_for_compare(child);
    let parent = canonical_for_compare(parent);
    child == parent || child.starts_with(&parent)
}

/// Snapshot-relative paths must stay inside the snapshot directory.
pub fn validate_snapshot_relative(path: &Path) -> Result<()> {
    if path.is_absolute() {
        return Err(Error::Config(format!(
            "存档内路径必须是相对路径：{}",
            path.display()
        )));
    }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::Config(format!(
                    "存档内路径不能越出快照目录：{}",
                    path.display()
                )));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    if path.as_os_str().is_empty() {
        return Err(Error::Config("存档内路径不能为空".to_owned()));
    }
    Ok(())
}

/// Manifest paths are also constrained to a release directory.
pub fn validate_release_relative(path: &Path) -> Result<()> {
    validate_snapshot_relative(path).map_err(|error| match error {
        Error::Config(message) => Error::Verify(message),
        other => other,
    })
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path).map_err(|source| Error::io(path, source))?;
    serde_json::from_slice(&bytes).map_err(|source| Error::json(path, source))
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::io(parent, source))?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::json(path, source))?;
    std::fs::write(path, bytes).map_err(|source| Error::io(path, source))
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::io(parent, source))?;
    }
    std::fs::write(path, bytes).map_err(|source| Error::io(path, source))
}

pub fn valid_release_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && id != "."
        && id != ".."
}

pub fn valid_dimension_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn floor_div(value: i32, divisor: i32) -> i32 {
    let quotient = value / divisor;
    let remainder = value % divisor;
    if remainder != 0 && (remainder < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::floor_div;

    #[test]
    fn tile_indices_use_floor_division_across_the_origin() {
        assert_eq!(floor_div(-1, 256), -1);
        assert_eq!(floor_div(0, 256), 0);
        assert_eq!(floor_div(255, 256), 0);
        assert_eq!(floor_div(256, 256), 1);
        assert_eq!(floor_div(-256, 256), -1);
        assert_eq!(floor_div(-257, 256), -2);
    }
}
