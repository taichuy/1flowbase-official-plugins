use agent_logs_collector::cli::{Cli, Command};
use anyhow::{bail, Result};
use opencode_logs_collector::{hooks, OpenCodeAdapter};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let Some(cli) = Cli::parse("opencode-logs-collector", env!("CARGO_PKG_VERSION"))? else {
        return Ok(());
    };
    let default_source = cli
        .config_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("sessions");
    let adapter = OpenCodeAdapter;
    if cli.run_common(&adapter, &default_source).await? {
        return Ok(());
    }
    match cli.command {
        Command::InstallHooks | Command::UninstallHooks => {
            let home = cli.client_home.clone().unwrap_or_else(|| {
                std::env::var_os("OPENCODE_CONFIG_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        let base = std::env::var_os("XDG_CONFIG_HOME")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| {
                                PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                                    .join(".config")
                            });
                        base.join("opencode")
                    })
            });
            hooks::register(
                &cli.config_path,
                &home,
                matches!(cli.command, Command::InstallHooks),
            )
        }
        _ => bail!("Unsupported OpenCode collector command"),
    }
}
