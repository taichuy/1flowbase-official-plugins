//! Text-preserving Cordis home patch registration.
use agent_logs_collector::{atomic_write, hash, CheckpointLock};
use anyhow::{ensure, Result};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
const BEGIN: &str = "# BEGIN 1flowbase deepseek-harness logs collector";
const END: &str = "# END 1flowbase deepseek-harness logs collector";
pub const PLUGIN: &str = include_str!("../native/plugin.mjs");
fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    })
}
fn remove_owned(text: &str) -> Result<String> {
    let starts: Vec<_> = text.match_indices(BEGIN).collect();
    let ends: Vec<_> = text.match_indices(END).collect();
    ensure!(
        starts.len() == ends.len() && starts.len() <= 1,
        "Ambiguous managed collector patch; preserving existing file"
    );
    if starts.is_empty() {
        return Ok(text.to_owned());
    }
    let start = starts[0].0;
    let end = ends[0].0 + END.len();
    ensure!(
        start == 0 || text.as_bytes()[start - 1] == b'\n',
        "Invalid managed block"
    );
    ensure!(
        end > start
            && (end == text.len()
                || text.as_bytes()[end] == b'\n'
                || text.as_bytes()[end] == b'\r'),
        "Invalid managed block"
    );
    let end = if text[end..].starts_with("\r\n") {
        end + 2
    } else if text[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };
    Ok(format!("{}{}", &text[..start], &text[end..]))
}
pub fn register(config: &Path, client_home: &Path, install: bool) -> Result<()> {
    let home = absolute(client_home)?;
    let config = absolute(config)?;
    let dir = home.join("1flowbase-logs");
    let patch = home.join("cordis.patch.yml");
    // SDK locks/private atomic writes reject symlinks throughout the owned path.
    let _lock = CheckpointLock::acquire(&dir.join("registration-state"))?;
    let text = match fs::read_to_string(&patch) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => anyhow::bail!("Cannot read Cordis patch; preserving file"),
    };
    let mut preserved = remove_owned(&text)?;
    if install {
        // YAML patch lists may carry !!js expressions; preserve their exact bytes.
        // JSON arrays cannot accept an appended YAML row. Refuse instead of rewriting.
        ensure!(
            !preserved.trim_start().starts_with('['),
            "JSON patch arrays require manual overlay registration; existing file preserved"
        );
        ensure!(
            preserved.trim().is_empty()
                || preserved
                    .lines()
                    .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                    .is_none_or(|line| line.trim_start().starts_with('-')),
            "Expected YAML patch list; existing file preserved"
        );
        let plugin = dir.join("plugin.mjs");
        if plugin.exists() {
            let previous = fs::read_to_string(&plugin)?;
            ensure!(
                previous.starts_with("// Native Cordis lifecycle integration;"),
                "Unowned plugin file; preserving it"
            );
            let receipt = fs::read_to_string(dir.join("plugin.sha256")).unwrap_or_default();
            ensure!(
                hash(previous.as_bytes()) == receipt.trim(),
                "Modified plugin file; preserving it"
            );
        }
        atomic_write(&plugin, PLUGIN.as_bytes())?;
        let binary = std::env::current_exe()?;
        let row = json!({"id":"1flowbase-deepseek-harness-logs","name":plugin,
            "config":{"binary":binary,"collectorConfig":config}});
        if !preserved.is_empty() && !preserved.ends_with('\n') {
            preserved.push('\n');
        }
        preserved.push_str(&format!("{BEGIN}\n- insert:\n    - {}\n{END}\n", row));
        atomic_write(&patch, preserved.as_bytes())?;
        atomic_write(&dir.join("plugin.sha256"), hash(PLUGIN).as_bytes())?;
    } else {
        if preserved != text {
            atomic_write(&patch, preserved.as_bytes())?;
        }
        let plugin = dir.join("plugin.mjs");
        let receipt = dir.join("plugin.sha256");
        if let (Ok(bytes), Ok(expected)) = (fs::read(&plugin), fs::read_to_string(&receipt)) {
            if hash(&bytes) == expected.trim() {
                fs::remove_file(&plugin)?;
                fs::remove_file(&receipt)?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_block_removal_preserves_native_expressions() {
        let original = "# user config\n- id: llm\n  config:\n    key: !!js process.env.TEST_KEY\n";
        let appended =
            format!("{original}{BEGIN}\n- insert:\n    - {{\"name\":\"owned\"}}\n{END}\n");
        assert_eq!(remove_owned(&appended).unwrap(), original);
        assert_eq!(remove_owned(original).unwrap(), original);
        assert!(remove_owned(&format!("{BEGIN}\n")).is_err());
        assert!(remove_owned(&format!("{BEGIN}\n{END}\n{BEGIN}\n{END}\n")).is_err());
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    #[test]
    fn install_upgrade_and_uninstall_preserve_unrelated_patch_and_modified_plugin() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home =
            std::env::temp_dir().join(format!("dsh-registration-{}-{unique}", std::process::id()));
        fs::create_dir_all(&home).unwrap();
        let patch = home.join("cordis.patch.yml");
        let original =
            "# unrelated\n- id: provider\n  config:\n    key: !!js process.env.TEST_KEY\n";
        fs::write(&patch, original).unwrap();
        register(&home.join("config.json"), &home, true).unwrap();
        register(&home.join("config.json"), &home, true).unwrap();
        let installed = fs::read_to_string(&patch).unwrap();
        assert_eq!(installed.matches(BEGIN).count(), 1);
        assert!(installed.starts_with(original));
        let plugin = home.join("1flowbase-logs/plugin.mjs");
        fs::write(&plugin, format!("{PLUGIN}\n// user customization\n")).unwrap();
        assert!(register(&home.join("config.json"), &home, true).is_err());
        register(&home.join("config.json"), &home, false).unwrap();
        assert_eq!(fs::read_to_string(&patch).unwrap(), original);
        assert!(plugin.exists());
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn json_array_and_foreign_plugin_remain_unmodified() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!(
            "dsh-registration-json-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(home.join("1flowbase-logs")).unwrap();
        fs::write(home.join("cordis.patch.yml"), "[]").unwrap();
        assert!(register(&home.join("config.json"), &home, true).is_err());
        assert_eq!(
            fs::read_to_string(home.join("cordis.patch.yml")).unwrap(),
            "[]"
        );
        fs::write(home.join("cordis.patch.yml"), "- id: native\n").unwrap();
        fs::write(home.join("1flowbase-logs/plugin.mjs"), "// other plugin").unwrap();
        assert!(register(&home.join("config.json"), &home, true).is_err());
        assert_eq!(
            fs::read_to_string(home.join("cordis.patch.yml")).unwrap(),
            "- id: native\n"
        );
        fs::remove_dir_all(home).unwrap();
    }
}
