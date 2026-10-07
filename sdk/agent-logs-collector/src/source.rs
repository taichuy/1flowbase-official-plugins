use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

pub fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
pub struct Position {
    pub line: Value,
    pub start: u64,
    pub end: u64,
    pub bytes: Vec<u8>,
}

pub(crate) fn reject_symlink_path(path: &Path) -> Result<()> {
    let mut part = PathBuf::new();
    for component in path.components() {
        part.push(component.as_os_str());
        match fs::symlink_metadata(&part) {
            Ok(meta) => ensure!(
                !meta.file_type().is_symlink(),
                "Symbolic links are not allowed in collector paths"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("Cannot inspect collector path"),
        }
    }
    Ok(())
}
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    reject_symlink_path(path)?;
    let parent = path.parent().context("State requires parent directory")?;
    fs::create_dir_all(parent).context("Cannot create collector directory")?;
    let temporary = parent.join(format!(".collector-{}.tmp", uuid::Uuid::new_v4()));
    let outcome = (|| -> Result<()> {
        let mut file = private_options()
            .open(&temporary)
            .context("Cannot create private collector file")?;
        file.write_all(bytes)
            .context("Cannot write collector file")?;
        file.sync_all().context("Cannot sync collector file")?;
        drop(file);
        // Windows rename does not replace an existing file. ReplaceFileW preserves atomicity.
        replace(&temporary, path)?;
        #[cfg(unix)]
        File::open(parent)
            .context("Cannot open collector directory")?
            .sync_all()
            .context("Cannot sync collector directory")?;
        Ok(())
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    outcome
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).context("Cannot atomically replace collector file")
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
    ensure!(
        unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 9) } != 0,
        "Cannot atomically replace collector file"
    );
    Ok(())
}
/// Advisory OS lock survives stale lock files and is released on process death.
pub struct CheckpointLock {
    _file: File,
}
impl CheckpointLock {
    pub fn acquire(state: &Path) -> Result<Self> {
        reject_symlink_path(state)?;
        let parent = state.parent().context("State requires parent directory")?;
        fs::create_dir_all(parent).context("Cannot create state directory")?;
        let mut name = state.as_os_str().to_owned();
        name.push(".lock");
        let path = PathBuf::from(name);
        reject_symlink_path(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).context("Cannot open checkpoint lock")?;
        file.try_lock()
            .map_err(|_| anyhow::anyhow!("Checkpoint is locked by another collector"))?;
        Ok(Self { _file: file })
    }
}
pub(crate) fn files(root: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    let meta = match fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => bail!("Cannot inspect source"),
    };
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_file() {
        if root.extension().is_some_and(|ext| ext == "jsonl") {
            output.push(root.to_owned());
        }
    } else if meta.is_dir() {
        let mut entries = fs::read_dir(root)
            .context("Cannot list source directory")?
            .collect::<std::io::Result<Vec<_>>>()
            .context("Cannot list source entry")?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            files(&entry.path(), output)?;
        }
    }
    Ok(())
}
pub(crate) struct Lines {
    reader: BufReader<File>,
    offset: u64,
}
impl Lines {
    pub fn open(file: &Path) -> Result<Self> {
        reject_symlink_path(file)?;
        Ok(Self {
            reader: BufReader::new(File::open(file).context("Cannot open source")?),
            offset: 0,
        })
    }
}
impl Iterator for Lines {
    type Item = Result<Position>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let mut bytes = Vec::new();
            match self.reader.read_until(b'\n', &mut bytes) {
                Ok(0) => return None,
                Ok(_) if bytes.last() != Some(&b'\n') => return None, // Only committed newline records.
                Ok(_) => {}
                Err(_) => return Some(Err(anyhow::anyhow!("Cannot read source record"))),
            }
            let start = self.offset;
            self.offset += bytes.len() as u64;
            bytes.pop();
            if bytes.iter().all(|c| c.is_ascii_whitespace()) {
                continue;
            }
            let line = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(_) => {
                    return Some(Err(anyhow::anyhow!("Invalid JSONL record at byte {start}")))
                }
            };
            return Some(Ok(Position {
                line,
                start,
                end: self.offset,
                bytes,
            }));
        }
    }
}
