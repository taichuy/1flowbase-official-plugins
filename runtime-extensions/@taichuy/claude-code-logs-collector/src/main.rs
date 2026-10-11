use agent_logs_collector::cli::{Cli, Command};
use anyhow::{bail, Result};
use claude_code_logs_collector::{hooks, ClaudeAdapter};
use std::path::PathBuf;
fn home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return hooks::absolute(&PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("Cannot determine user home"))?;
    hooks::absolute(&PathBuf::from(home).join(".claude"))
}
async fn run() -> Result<()> {
    let Some(cli) = Cli::parse("claude-code-logs-collector", env!("CARGO_PKG_VERSION"))? else {
        return Ok(());
    };
    let home = cli
        .client_home
        .clone()
        .map(|p| hooks::absolute(&p))
        .transpose()?
        .unwrap_or(home()?);
    let adapter = ClaudeAdapter {
        client_home: home.clone(),
    };
    if cli.run_common(&adapter, &home).await? {
        return Ok(());
    }
    match cli.command {
        Command::InstallHooks => hooks::register(&cli.config_path, &home, true)?,
        Command::UninstallHooks => hooks::register(&cli.config_path, &home, false)?,
        Command::Hook => {
            let config = agent_logs_collector::Config::load(&cli.config_path)?;
            hooks::run(&config, &adapter, cli.options()?).await?;
            println!("{{}}");
        }
        _ => bail!("Unsupported Claude command"),
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
