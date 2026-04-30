use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::{UnderlayError, UnderlayResult};

pub fn atomic_write(
    path: &Path,
    payload: &[u8],
    io_error: impl Fn(std::io::Error) -> UnderlayError + Copy,
) -> UnderlayResult<()> {
    let parent = path.parent().ok_or_else(|| {
        UnderlayError::Internal(format!("atomic write path {:?} has no parent", path))
    })?;
    fs::create_dir_all(parent).map_err(io_error)?;

    let tmp_path = unique_tmp_path(path)?;
    let result = write_then_rename(path, &tmp_path, payload, io_error);
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn write_then_rename(
    path: &Path,
    tmp_path: &Path,
    payload: &[u8],
    io_error: impl Fn(std::io::Error) -> UnderlayError + Copy,
) -> UnderlayResult<()> {
    let mut tmp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp_path)
        .map_err(io_error)?;
    tmp_file.write_all(payload).map_err(io_error)?;
    tmp_file.sync_all().map_err(io_error)?;
    drop(tmp_file);

    fs::rename(tmp_path, path).map_err(io_error)?;
    sync_parent_dir(path, io_error)
}

fn unique_tmp_path(path: &Path) -> UnderlayResult<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| UnderlayError::Internal(format!("invalid atomic write path {:?}", path)))?;
    Ok(path.with_file_name(format!(
        ".{file_name}.{}.tmp",
        Uuid::new_v4()
    )))
}

fn sync_parent_dir(
    path: &Path,
    io_error: impl Fn(std::io::Error) -> UnderlayError + Copy,
) -> UnderlayResult<()> {
    let parent = path.parent().ok_or_else(|| {
        UnderlayError::Internal(format!("atomic write path {:?} has no parent", path))
    })?;
    File::open(parent).and_then(|dir| dir.sync_all()).map_err(io_error)
}
