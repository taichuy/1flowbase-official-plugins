use agent_logs_collector::cli::{Cli, Command};
use anyhow::{bail, Result};
use deepseek_harness_logs_collector::{hooks, DeepSeekHarnessAdapter};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("deepseek-harness-logs-collector: {error:#}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let Some(cli) = Cli::parse("deepseek-harness-logs-collector", env!("CARGO_PKG_VERSION"))?
    else {
        return Ok(());
    };
    let home = cli.client_home.clone().unwrap_or_else(|| {
        std::env::var_os("DSH_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(
                    std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .unwrap_or_default(),
                )
                .join(".dsh")
            })
    });
    let default_source = home.join("1flowbase-logs").join("journal");
    if cli
        .run_common(&DeepSeekHarnessAdapter, &default_source)
        .await?
    {
        return Ok(());
    }
    match cli.command {
        Command::InstallHooks => hooks::register(&cli.config_path, &home, true),
        Command::UninstallHooks => hooks::register(&cli.config_path, &home, false),
        _ => bail!("Unsupported DeepSeek Harness command"),
    }
}
