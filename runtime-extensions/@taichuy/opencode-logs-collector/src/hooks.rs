use agent_logs_collector::{atomic_write, hash, CheckpointLock, Config};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

const TEMPLATE: &str = include_str!("plugin.js");
fn absolute(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute(),
        "Client home and collector config must be absolute"
    );
    ensure!(
        !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "Parent components are not allowed"
    );
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(
                !meta.file_type().is_symlink(),
                "Symbolic links are not allowed"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e).context("Cannot inspect installation path"),
        }
    }
    Ok(path.to_owned())
}
pub fn render(binary: &Path, config: &Path) -> Result<String> {
    Ok(TEMPLATE
        .replace(
            "__COLLECTOR_BINARY__",
            &serde_json::to_string(&binary.to_string_lossy())?,
        )
        .replace(
            "__COLLECTOR_CONFIG__",
            &serde_json::to_string(&config.to_string_lossy())?,
        ))
}
/// Own only the uniquely named local plugin file; never change OpenCode config.
pub fn register(config: &Path, home: &Path, install: bool) -> Result<()> {
    let config = absolute(config)?;
    let home = absolute(home)?;
    if install {
        Config::load(&config)?;
    }
    let tag = hash(config.to_string_lossy().as_bytes());
    let marker = format!("// 1flowbase OpenCode collector {tag}\n");
    let path = home
        .join("plugins")
        .join(format!("1flowbase-agent-logs-{tag}.js"));
    absolute(&path)?;
    let _lock = CheckpointLock::acquire(&path)?;
    let existing = match fs::read_to_string(&path) {
        Ok(value) => Some(value),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("Cannot read installed OpenCode plugin"),
    };
    if let Some(value) = &existing {
        ensure!(
            value.starts_with(&marker),
            "Plugin ownership marker differs; left unchanged"
        );
    }
    if install {
        let binary = std::env::current_exe().context("Cannot locate collector binary")?;
        atomic_write(
            &path,
            format!("{marker}{}", render(&binary, &config)?).as_bytes(),
        )?;
    } else if existing.is_some() {
        fs::remove_file(&path).context("Cannot remove owned OpenCode plugin")?;
    }
    Ok(())
}
