use agent_logs_collector::{collect, configure, Config};
use anyhow::{ensure, Result};
use codex_logs_collector::CodexAdapter;
use std::{collections::BTreeMap, io::Read, path::PathBuf, time::Duration};
const HELP: &str = "codex-logs-collector 0.1.0\n\nconfigure --endpoint URL --source PATH --config PATH [--key-stdin]\nimport --config PATH\nwatch --config PATH\n--help | --version\n\nKey: --key-stdin or FLOWBASE_AGENT_LOGS_API_KEY. Default source: CODEX_HOME or ~/.codex.\nConfig credentials are private; source/state identity is preserved on upgrades.\n";
fn codex_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_HOME") {
        return absolute(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("Cannot determine user home"))?;
    absolute(PathBuf::from(home).join(".codex"))
}
fn absolute(path: PathBuf) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            p => normalized.push(p.as_os_str()),
        }
    }
    Ok(normalized)
}
fn options(args: &[String], command: &str) -> Result<(BTreeMap<String, String>, bool)> {
    let mut values = BTreeMap::new();
    let mut key_stdin = false;
    let mut index = 0;
    while index < args.len() {
        let option = &args[index];
        if command == "configure" && option == "--key-stdin" {
            ensure!(!key_stdin, "Duplicate option");
            key_stdin = true;
            index += 1;
            continue;
        }
        let allowed = option == "--config"
            || (command == "configure" && matches!(option.as_str(), "--endpoint" | "--source"));
        ensure!(allowed, "Unknown option; use --help");
        let value = args
            .get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| anyhow::anyhow!("Option requires a value"))?;
        ensure!(
            values.insert(option.clone(), value.clone()).is_none(),
            "Duplicate option"
        );
        index += 2;
    }
    Ok((values, key_stdin))
}
#[cfg(unix)]
async fn shutdown() {
    if let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
}
#[cfg(not(unix))]
async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        print!("{HELP}");
        return Ok(());
    }
    if args == ["--version"] {
        println!("codex-logs-collector 0.1.0");
        return Ok(());
    }
    let command = args[0].as_str();
    ensure!(
        matches!(command, "configure" | "import" | "watch"),
        "Unknown command; use --help"
    );
    let (values, key_stdin) = options(&args[1..], command)?;
    let path = values
        .get("--config")
        .ok_or_else(|| anyhow::anyhow!("--config is required"))?;
    let home = codex_home()?;
    if command == "configure" {
        let endpoint = values
            .get("--endpoint")
            .ok_or_else(|| anyhow::anyhow!("--endpoint is required"))?
            .clone();
        let source = values
            .get("--source")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.clone());
        let key = if key_stdin {
            let mut value = String::new();
            std::io::stdin()
                .read_to_string(&mut value)
                .map_err(|_| anyhow::anyhow!("Cannot read API key from stdin"))?;
            value.trim_end_matches(['\r', '\n']).to_owned()
        } else {
            std::env::var("FLOWBASE_AGENT_LOGS_API_KEY")
                .map_err(|_| anyhow::anyhow!("Use --key-stdin or FLOWBASE_AGENT_LOGS_API_KEY"))?
        };
        configure(&PathBuf::from(path), endpoint, &source, key)?;
        println!("Collector configured");
        return Ok(());
    }
    let config = Config::load(&PathBuf::from(path))?;
    let adapter = CodexAdapter { codex_home: home };
    if command == "import" {
        let report = tokio::select! { result = collect(&config, &adapter) => result?, _ = shutdown() => { return Ok(()); } };
        println!(
            "Uploaded {} events; {} files await explicit turn ownership",
            report.uploaded, report.unattributed_files
        );
        return Ok(());
    }
    let stop = shutdown();
    tokio::pin!(stop);
    let mut backoff = 1u64;
    loop {
        let outcome = tokio::select! { result = collect(&config, &adapter) => result, _ = &mut stop => return Ok(()) };
        let delay = match outcome {
            Ok(report) => {
                if report.uploaded > 0 {
                    println!("Uploaded {} events", report.uploaded);
                }
                backoff = 1;
                1
            }
            Err(error) => {
                eprintln!("{error}");
                let delay = backoff;
                backoff = (backoff * 2).min(30);
                delay
            }
        };
        tokio::select! { _ = tokio::time::sleep(Duration::from_secs(delay)) => {}, _ = &mut stop => return Ok(()) }
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
