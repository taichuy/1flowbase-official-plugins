use agent_logs_collector::cli::{Cli, Command};
use anyhow::{bail, Result};
use antigravity_logs_collector::{hooks, AntigravityAdapter};
use std::{path::PathBuf, time::Duration};

fn default_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("GEMINI_HOME") {
        return hooks::absolute(&PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("Cannot determine user home"))?;
    hooks::absolute(&PathBuf::from(home).join(".gemini"))
}

async fn run() -> Result<()> {
    let Some(cli) = Cli::parse("antigravity-logs-collector", env!("CARGO_PKG_VERSION"))? else {
        return Ok(());
    };
    let home = cli
        .client_home
        .clone()
        .map(|p| hooks::absolute(&p))
        .transpose()?
        .unwrap_or(default_home()?);
    let parent_links = cli
        .config_path
        .parent()
        .map(|dir| dir.join("subagent-parents.json"));
    let adapter = AntigravityAdapter::new(home.clone(), parent_links);

    if cli.command == Command::Flush {
        if let Ok(raw) = std::env::var("FLOWBASE_COLLECTOR_SETTLE_MS") {
            if let Ok(ms) = raw.parse::<u64>() {
                if ms > 0 {
                    tokio::time::sleep(Duration::from_millis(ms.min(2000))).await;
                }
            }
        }
    }

    if cli.run_common(&adapter, &home).await? {
        return Ok(());
    }
    match cli.command {
        Command::InstallHooks => hooks::register(&cli.config_path, &home, true)?,
        Command::UninstallHooks => hooks::register(&cli.config_path, &home, false)?,
        Command::Hook => {
            let config = agent_logs_collector::Config::load(&cli.config_path)?;
            hooks::run(&config, &cli.config_path, &adapter, cli.options()?).await?;
            println!("{{}}");
        }
        _ => bail!("Unsupported Antigravity collector command"),
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
