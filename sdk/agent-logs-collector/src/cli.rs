//! Shared short-lived collector commands. Client packages own native registration.
use crate::{
    append_record, collect_pending, collect_with_options, configure, CollectOptions, Config,
    SourceAdapter,
};
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Configure,
    Import,
    Flush,
    Record,
    Hook,
    InstallHooks,
    UninstallHooks,
}

pub struct Cli {
    pub command: Command,
    pub config_path: PathBuf,
    pub client_home: Option<PathBuf>,
    values: BTreeMap<String, String>,
    key_stdin: bool,
    flush: bool,
}
impl Cli {
    pub fn parse(code: &str, version: &str) -> Result<Option<Self>> {
        Self::parse_args(code, version, std::env::args().skip(1).collect())
    }
    fn parse_args(code: &str, version: &str, args: Vec<String>) -> Result<Option<Self>> {
        if args.is_empty() || args == ["--help"] || args == ["-h"] {
            println!("{code} {version}\n\nconfigure --endpoint URL --source PATH --config PATH [--key-stdin]\nimport|flush --config PATH [--batch-bytes BYTES]\nhook --config PATH (native JSON on stdin)\nrecord --config PATH --session ID [--flush] (native JSON on stdin)\ninstall-hooks|uninstall-hooks --config PATH [--client-home PATH]\n\nConfiguration and registration do not import history. Keys: stdin or FLOWBASE_AGENT_LOGS_API_KEY.");
            return Ok(None);
        }
        if args == ["--version"] {
            println!("{code} {version}");
            return Ok(None);
        }
        let command = match args[0].as_str() {
            "configure" => Command::Configure,
            "import" => Command::Import,
            "flush" => Command::Flush,
            "record" => Command::Record,
            "hook" => Command::Hook,
            "install-hooks" => Command::InstallHooks,
            "uninstall-hooks" => Command::UninstallHooks,
            _ => anyhow::bail!("Unknown command; use --help"),
        };
        let mut values = BTreeMap::new();
        let mut key_stdin = false;
        let mut flush = false;
        let mut index = 1;
        while index < args.len() {
            let option = args[index].as_str();
            if option == "--key-stdin" && command == Command::Configure {
                ensure!(!key_stdin, "Duplicate option");
                key_stdin = true;
                index += 1;
                continue;
            }
            if option == "--flush" && command == Command::Record {
                ensure!(!flush, "Duplicate option");
                flush = true;
                index += 1;
                continue;
            }
            let allowed = option == "--config"
                || command == Command::Configure && matches!(option, "--endpoint" | "--source")
                || matches!(
                    command,
                    Command::Import | Command::Flush | Command::Record | Command::Hook
                ) && option == "--batch-bytes"
                || command == Command::Record && option == "--session"
                || matches!(command, Command::InstallHooks | Command::UninstallHooks)
                    && option == "--client-home";
            ensure!(allowed, "Unknown option; use --help");
            let value = args
                .get(index + 1)
                .filter(|v| !v.starts_with("--"))
                .context("Option requires a value")?;
            ensure!(
                values.insert(option.to_owned(), value.clone()).is_none(),
                "Duplicate option"
            );
            index += 2;
        }
        let config_path = PathBuf::from(values.get("--config").context("--config is required")?);
        let client_home = values.get("--client-home").map(PathBuf::from);
        Ok(Some(Self {
            command,
            config_path,
            client_home,
            values,
            key_stdin,
            flush,
        }))
    }
    pub fn options(&self) -> Result<CollectOptions> {
        let mut options = CollectOptions::default();
        if let Some(bytes) = self.values.get("--batch-bytes") {
            options.batch_target_bytes = bytes.parse().context("--batch-bytes must be positive")?;
            ensure!(
                options.batch_target_bytes > 0,
                "--batch-bytes must be positive"
            );
        }
        Ok(options)
    }
    pub async fn run_common<A: SourceAdapter>(
        &self,
        adapter: &A,
        default_source: &Path,
    ) -> Result<bool> {
        match self.command {
            Command::Configure => {
                let endpoint = self
                    .values
                    .get("--endpoint")
                    .context("--endpoint is required")?;
                let source = self
                    .values
                    .get("--source")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| default_source.to_owned());
                let key = if self.key_stdin {
                    let mut key = String::new();
                    std::io::stdin()
                        .read_to_string(&mut key)
                        .context("Cannot read key from stdin")?;
                    key.trim_end_matches(['\r', '\n']).to_owned()
                } else {
                    std::env::var("FLOWBASE_AGENT_LOGS_API_KEY")
                        .context("Use --key-stdin or FLOWBASE_AGENT_LOGS_API_KEY")?
                };
                configure(&self.config_path, endpoint.clone(), &source, key)?;
                println!("Collector configured");
            }
            Command::Import => {
                let config = Config::load(&self.config_path)?;
                let report = collect_with_options(&config, adapter, self.options()?).await?;
                println!(
                    "Uploaded {} events; {} sources await task attribution",
                    report.uploaded, report.unattributed_files
                );
            }
            Command::Flush | Command::Record => {
                let config = Config::load(&self.config_path)?;
                if self.command == Command::Record {
                    let session = self
                        .values
                        .get("--session")
                        .context("--session is required")?;
                    let value = serde_json::from_reader(std::io::stdin().lock())
                        .context("Invalid native event JSON")?;
                    append_record(&config, adapter, session, &value)?;
                    if !self.flush {
                        return Ok(true);
                    }
                }
                let report = collect_pending(&config, adapter, self.options()?).await?;
                println!(
                    "Uploaded {} events; {} sources pending",
                    report.uploaded, report.pending_sources
                );
                ensure!(
                    report.pending_sources == 0,
                    "Delivery deferred; sources retained for retry"
                );
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
