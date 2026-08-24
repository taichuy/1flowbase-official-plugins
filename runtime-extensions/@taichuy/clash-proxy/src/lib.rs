use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

mod runtime_pool;

use runtime_pool::{ManagedRuntime, PoolError, RuntimePool};

use anyhow::{anyhow, bail, Context, Result};
use rand::{rngs::OsRng, RngCore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const CONTRACT_VERSION: &str = "1flowbase.network_egress_provider/v1";
const LEASE_DURATION_MILLIS: u64 = 300_000;
const CORE_READY_TIMEOUT: Duration = Duration::from_secs(5);
const SUBSCRIPTION_USER_AGENT: &str = "clash.meta";
const SUBSCRIPTION_UNAVAILABLE_CODE: &str = "network_egress_subscription_unavailable";
const SUBSCRIPTION_INVALID_CODE: &str = "network_egress_subscription_invalid";
const PROXY_INVALID_CODE: &str = "network_egress_proxy_invalid";
const RUNTIME_START_FAILED_CODE: &str = "network_egress_runtime_start_failed";
const RUNTIME_RESOURCE_EXHAUSTED_CODE: &str = "network_egress_runtime_resource_exhausted";
const RUNTIME_CAPACITY_EXHAUSTED_CODE: &str = "network_egress_runtime_capacity_exhausted";
const RUNTIME_BACKOFF_CODE: &str = "network_egress_runtime_backoff";
const MAX_ACTIVE_RUNTIMES: usize = 4;
const IDLE_RUNTIME_TTL: Duration = Duration::from_secs(60);
const FAILED_RUNTIME_BACKOFF: Duration = Duration::from_secs(5);
const REAPER_INTERVAL: Duration = Duration::from_secs(1);
const SUPPORTED_REMOTE_PROXY_TYPES: &[&str] = &[
    "anytls",
    "gost-relay",
    "http",
    "hysteria",
    "hysteria2",
    "mieru",
    "shadowquic",
    "snell",
    "socks5",
    "ss",
    "ssr",
    "trojan",
    "tuic",
    "vless",
    "vmess",
];
static NEXT_LEASE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct EgressError {
    code: &'static str,
    message: &'static str,
}

impl EgressError {
    const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for EgressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for EgressError {}

fn subscription_unavailable_error() -> anyhow::Error {
    EgressError::new(
        SUBSCRIPTION_UNAVAILABLE_CODE,
        "Subscription is unavailable.",
    )
    .into()
}

fn subscription_invalid_error() -> anyhow::Error {
    EgressError::new(SUBSCRIPTION_INVALID_CODE, "Subscription data is invalid.").into()
}

fn proxy_invalid_error() -> anyhow::Error {
    EgressError::new(PROXY_INVALID_CODE, "Proxy node is invalid.").into()
}

fn runtime_start_failed_error(
    message: &'static str,
    cause: impl std::error::Error + Send + Sync + 'static,
) -> anyhow::Error {
    anyhow::Error::new(cause).context(EgressError::new(RUNTIME_START_FAILED_CODE, message))
}

fn runtime_resource_exhausted_error(cause: anyhow::Error) -> anyhow::Error {
    cause.context(EgressError::new(
        RUNTIME_RESOURCE_EXHAUSTED_CODE,
        "Proxy runtime could not reserve required memory.",
    ))
}

fn runtime_capacity_exhausted_error() -> anyhow::Error {
    EgressError::new(
        RUNTIME_CAPACITY_EXHAUSTED_CODE,
        "Proxy runtime capacity is exhausted.",
    )
    .into()
}

fn runtime_backoff_error() -> anyhow::Error {
    EgressError::new(
        RUNTIME_BACKOFF_CODE,
        "Proxy runtime startup is temporarily paused.",
    )
    .into()
}

#[derive(Debug, Clone)]
pub struct ProxyEntry {
    pub name: String,
    pub kind: String,
    config: Value,
}

#[derive(Debug, Clone)]
pub struct Egress {
    pub key: String,
    pub display_name: String,
    pub proxy: ProxyEntry,
}

#[derive(Debug, Clone)]
pub struct FixedYamlV1 {
    egresses: Vec<Egress>,
}

impl FixedYamlV1 {
    /// The host's startup-only secret carrier is JSON. The subscription URL is read only by the
    /// provider. The provider fetches a Clash-compatible YAML subscription over HTTPS and
    /// projects only its remote proxy nodes into an isolated Mihomo configuration.
    pub fn from_secret_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).map_err(|_| subscription_unavailable_error())?;
        let secret = serde_json::from_str::<serde_json::Value>(&raw)
            .map_err(|_| subscription_invalid_error())?;
        let subscription_url = secret
            .get("subscription_url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| value.starts_with("https://") && value.len() <= 2048)
            .ok_or_else(subscription_invalid_error)?;
        let mut response = subscription_agent()
            .get(subscription_url)
            .header("User-Agent", SUBSCRIPTION_USER_AGENT)
            .call()
            .map_err(|_| subscription_unavailable_error())?;
        let yaml = response
            .body_mut()
            .read_to_string()
            .map_err(|_| subscription_unavailable_error())?;
        if yaml.len() > 1_048_576 {
            return Err(subscription_invalid_error());
        }
        Self::from_yaml(&yaml)
    }

    pub fn from_yaml(raw: &str) -> Result<Self> {
        if raw.trim().is_empty() || looks_like_base64(raw) {
            return Err(subscription_invalid_error());
        }
        let root: serde_yaml::Mapping =
            serde_yaml::from_str(raw).map_err(|_| subscription_invalid_error())?;
        let proxies = root
            .get(serde_yaml::Value::String("proxies".to_owned()))
            .and_then(serde_yaml::Value::as_sequence)
            .ok_or_else(subscription_invalid_error)?;
        if proxies.is_empty() {
            return Err(subscription_invalid_error());
        }

        let mut egresses = Vec::with_capacity(proxies.len());
        for value in proxies {
            let proxy = parse_proxy(value)?;
            let key = provider_egress_key(&proxy)?;
            egresses.push(Egress {
                key,
                display_name: proxy.name.clone(),
                proxy,
            });
        }
        egresses.sort_by(|left, right| left.key.cmp(&right.key));
        if egresses.windows(2).any(|pair| pair[0].key == pair[1].key) {
            return Err(proxy_invalid_error());
        }
        Ok(Self { egresses })
    }

    pub fn egresses(&self) -> &[Egress] {
        &self.egresses
    }

    fn egress(&self, key: &str) -> Result<&Egress> {
        self.egresses
            .iter()
            .find(|egress| egress.key == key)
            .ok_or_else(|| anyhow!("provider_egress_key is unavailable"))
    }
}

fn subscription_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        // A network-egress provider must not recursively depend on the host's ambient proxy.
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    ureq::Agent::new_with_config(config)
}

fn parse_proxy(value: &serde_yaml::Value) -> Result<ProxyEntry> {
    let object = value.as_mapping().ok_or_else(proxy_invalid_error)?;
    let mut fields = BTreeMap::new();
    for (key, value) in object {
        let key = key.as_str().ok_or_else(proxy_invalid_error)?;
        fields.insert(key, value);
    }
    let name = required_safe_text(&fields, "name")?;
    let kind = required_safe_text(&fields, "type")?.to_ascii_lowercase();
    let server = required_safe_text(&fields, "server")?;
    if server.contains("://") || server.contains('@') || server.contains('/') {
        return Err(proxy_invalid_error());
    }
    let _port = fields
        .get("port")
        .and_then(|value| value.as_u64())
        .filter(|port| (1..=65535).contains(port))
        .map(|port| port as u16)
        .ok_or_else(proxy_invalid_error)?;

    if !SUPPORTED_REMOTE_PROXY_TYPES.contains(&kind.as_str()) {
        return Err(proxy_invalid_error());
    }
    let config = serde_json::to_value(value).map_err(|_| proxy_invalid_error())?;
    Ok(ProxyEntry { name, kind, config })
}

fn required_safe_text(fields: &BTreeMap<&str, &serde_yaml::Value>, field: &str) -> Result<String> {
    optional_safe_text(fields, field)?.ok_or_else(proxy_invalid_error)
}

fn optional_safe_text(
    fields: &BTreeMap<&str, &serde_yaml::Value>,
    field: &str,
) -> Result<Option<String>> {
    let Some(value) = fields.get(field) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(proxy_invalid_error)?;
    if value.len() > 256 || value.contains('\n') || value.contains('\r') {
        return Err(proxy_invalid_error());
    }
    Ok(Some(value.to_owned()))
}

fn provider_egress_key(proxy: &ProxyEntry) -> Result<String> {
    let canonical_config = canonical_json(&proxy.config);
    let serialized = serde_json::to_vec(&canonical_config).map_err(|_| proxy_invalid_error())?;
    let digest = Sha256::digest(serialized);
    let fingerprint: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("clash/{fingerprint}"))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        Value::Object(fields) => {
            let mut canonical_fields = BTreeMap::new();
            for (key, value) in fields {
                canonical_fields.insert(key.clone(), canonical_json(value));
            }
            Value::Object(canonical_fields.into_iter().collect())
        }
        _ => value.clone(),
    }
}

fn looks_like_base64(value: &str) -> bool {
    let compact: String = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    compact.len() >= 80
        && compact.len() % 4 == 0
        && compact.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=')
        })
}

#[derive(Debug)]
struct MihomoRuntime {
    proxy_url: String,
    config_path: PathBuf,
    stderr_path: PathBuf,
    config_dir: PathBuf,
    child: Child,
}

impl MihomoRuntime {
    fn start(core_path: &Path, proxy: &ProxyEntry) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|cause| {
            runtime_start_failed_error("Proxy runtime could not reserve a local port.", cause)
        })?;
        let address = listener
            .local_addr()
            .map_err(|cause| {
                runtime_start_failed_error("Proxy runtime could not resolve its local port.", cause)
            })?
            .to_string();
        drop(listener);
        let (config_path, config_dir) = write_core_config(proxy, &address).map_err(|cause| {
            cause.context(EgressError::new(
                RUNTIME_START_FAILED_CODE,
                "Proxy runtime configuration could not be prepared.",
            ))
        })?;
        let stderr_path = config_dir.join("mihomo.stderr");
        let stderr = fs::File::create(&stderr_path).map_err(|cause| {
            cleanup_runtime_files(&config_path, &stderr_path, &config_dir);
            runtime_start_failed_error("Proxy runtime diagnostics could not be prepared.", cause)
        })?;
        let child = Command::new(core_path)
            .arg("-f")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(cause) => {
                cleanup_runtime_files(&config_path, &stderr_path, &config_dir);
                return Err(runtime_start_failed_error(
                    "Proxy runtime process could not be created.",
                    cause,
                ));
            }
        };
        if let Err(cause) = wait_for_loopback(&address, &mut child) {
            let _ = child.kill();
            let _ = child.wait();
            let resource_exhausted = stderr_reports_resource_exhaustion(&stderr_path);
            cleanup_runtime_files(&config_path, &stderr_path, &config_dir);
            let error = cause.context("Mihomo did not become ready");
            return Err(if resource_exhausted {
                runtime_resource_exhausted_error(error)
            } else {
                error.context(EgressError::new(
                    RUNTIME_START_FAILED_CODE,
                    "Proxy runtime did not become ready.",
                ))
            });
        }
        Ok(Self {
            proxy_url: format!("http://{address}"),
            config_path,
            stderr_path,
            config_dir,
            child,
        })
    }

    fn shutdown(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        cleanup_runtime_files(&self.config_path, &self.stderr_path, &self.config_dir);
    }
}

fn stderr_reports_resource_exhaustion(path: &Path) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file.take(16 * 1024).read_to_end(&mut bytes).is_err() {
        return false;
    }
    let diagnostic = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    [
        "failed to reserve page summary memory",
        "cannot allocate memory",
        "out of memory",
        "memory limit",
    ]
    .iter()
    .any(|marker| diagnostic.contains(marker))
}

fn cleanup_runtime_files(config_path: &Path, stderr_path: &Path, config_dir: &Path) {
    let _ = fs::remove_file(config_path);
    let _ = fs::remove_file(stderr_path);
    let _ = fs::remove_dir(config_dir);
}

impl ManagedRuntime for MihomoRuntime {
    fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn terminate(self) {
        self.shutdown();
    }
}

pub struct Worker {
    config: FixedYamlV1,
    core_path: PathBuf,
    pool: Arc<Mutex<RuntimePool<MihomoRuntime>>>,
    reaper_stop: Option<mpsc::Sender<()>>,
    reaper: Option<thread::JoinHandle<()>>,
}

impl Worker {
    pub fn start(config_path: &Path) -> Result<Self> {
        Self::new(
            FixedYamlV1::from_secret_file(config_path)?,
            bundled_core_path()?,
        )
    }

    fn new(config: FixedYamlV1, core_path: PathBuf) -> Result<Self> {
        let pool = Arc::new(Mutex::new(RuntimePool::new(
            MAX_ACTIVE_RUNTIMES,
            IDLE_RUNTIME_TTL.as_millis() as u64,
            FAILED_RUNTIME_BACKOFF.as_millis() as u64,
        )));
        let reaper_pool = pool.clone();
        let (reaper_stop, stop_receiver) = mpsc::channel();
        let reaper = thread::Builder::new()
            .name("clash-runtime-reaper".to_owned())
            .spawn(move || loop {
                match stop_receiver.recv_timeout(REAPER_INTERVAL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let Ok(mut pool) = reaper_pool.lock() else {
                            break;
                        };
                        pool.reap_idle(now_millis());
                    }
                }
            })
            .context("cannot start proxy runtime reaper")?;
        Ok(Self {
            config,
            core_path,
            pool,
            reaper_stop: Some(reaper_stop),
            reaper: Some(reaper),
        })
    }

    pub fn handle(&mut self, request: Value) -> Result<Value> {
        let operation = request
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("request operation is required"))?;
        match operation {
            "sync_egresses" => {
                ensure_exact_input(&request, &[])?;
                Ok(
                    json!({"operation":"sync_egresses","result":{"egresses": self.config.egresses().iter().map(|egress| json!({
                    "provider_egress_key": egress.key,
                    "display_name": egress.display_name,
                    "tags": [egress.proxy.kind],
                    "availability": "available"
                })).collect::<Vec<_>>()}}),
                )
            }
            "acquire_http_forward_proxy" => {
                ensure_exact_input(&request, &["provider_egress_key"])?;
                let key = request["input"]["provider_egress_key"]
                    .as_str()
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| anyhow!("provider_egress_key is required"))?;
                let lease = self.acquire(key)?;
                Ok(json!({"operation":"acquire_http_forward_proxy","result":{
                    "lease_id": lease.0,
                    "http_proxy_url": lease.1,
                    "cleanup_token": lease.2,
                    "expires_at": now_millis() + LEASE_DURATION_MILLIS
                }}))
            }
            "release_http_forward_proxy" => {
                ensure_exact_input(&request, &["lease_id", "cleanup_token"])?;
                let lease_id = required_input_string(&request, "lease_id")?;
                let cleanup_token = required_input_string(&request, "cleanup_token")?;
                self.release(lease_id, cleanup_token)?;
                Ok(
                    json!({"operation":"release_http_forward_proxy","result":{"lease_id": lease_id}}),
                )
            }
            _ => bail!("unsupported network egress operation"),
        }
    }

    fn acquire(&mut self, key: &str) -> Result<(String, String, String)> {
        let egress = self.config.egress(key)?.clone();
        let sequence = NEXT_LEASE.fetch_add(1, Ordering::Relaxed);
        let lease_id = format!("clash-{}-{sequence}", now_millis());
        let cleanup_token = random_token();
        let core_path = self.core_path.clone();
        let acquired = self
            .pool
            .lock()
            .map_err(|_| anyhow!("runtime pool lock is unavailable"))?
            .acquire(key, lease_id, cleanup_token, now_millis(), || {
                MihomoRuntime::start(&core_path, &egress.proxy)
            })
            .map_err(map_runtime_pool_error)?;
        Ok((
            acquired.lease_id,
            acquired.proxy_url,
            acquired.cleanup_token,
        ))
    }

    fn release(&mut self, lease_id: &str, cleanup_token: &str) -> Result<()> {
        self.pool
            .lock()
            .map_err(|_| anyhow!("runtime pool lock is unavailable"))?
            .release(lease_id, cleanup_token, now_millis())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(stop) = self.reaper_stop.take() {
            let _ = stop.send(());
        }
        if let Some(reaper) = self.reaper.take() {
            let _ = reaper.join();
        }
        if let Ok(mut pool) = self.pool.lock() {
            pool.shutdown();
        }
    }
}

fn map_runtime_pool_error(error: anyhow::Error) -> anyhow::Error {
    match error.downcast_ref::<PoolError>() {
        Some(PoolError::CapacityExhausted) => runtime_capacity_exhausted_error(),
        Some(PoolError::Backoff) => runtime_backoff_error(),
        None => error,
    }
}

fn ensure_exact_input(request: &Value, fields: &[&str]) -> Result<()> {
    let input = request
        .get("input")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("request input must be an object"))?;
    if input.len() != fields.len() || fields.iter().any(|field| !input.contains_key(*field)) {
        bail!("request input does not match the frozen network egress ABI");
    }
    Ok(())
}

fn required_input_string<'a>(request: &'a Value, field: &str) -> Result<&'a str> {
    request["input"][field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("{field} is required"))
}

fn bundled_core_path() -> Result<PathBuf> {
    let executable = std::env::current_exe().context("cannot resolve worker executable")?;
    let root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow!("worker executable has no package root"))?;
    let target = target_triple();
    let file = if cfg!(windows) {
        "1flowbase-runtime-core.exe"
    } else {
        "1flowbase-runtime-core"
    };
    let path = root.join("runtime-core").join(target).join(file);
    if !path.is_file() {
        bail!("bundled Mihomo runtime core is missing");
    }
    Ok(path)
}

fn target_triple() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-musl"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-musl"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "aarch64-pc-windows-msvc"
    } else {
        "unsupported"
    }
}

fn write_core_config(proxy: &ProxyEntry, address: &str) -> Result<(PathBuf, PathBuf)> {
    let directory = std::env::temp_dir().join(format!("1flowbase-clash-{}", random_token()));
    fs::create_dir(&directory).context("cannot create ephemeral core config directory")?;
    let config_path = directory.join("mihomo.yaml");
    let payload = json!({
        "mixed-port": address.rsplit(':').next().ok_or_else(|| anyhow!("invalid loopback address"))?.parse::<u16>()?,
        "bind-address": "127.0.0.1",
        "allow-lan": false,
        "mode": "global",
        "log-level": "warning",
        "ipv6": false,
        "proxies": [proxy.config],
        "proxy-groups": [{"name":"1flowbase-egress", "type":"select", "proxies":[proxy.name]}],
        "rules": ["MATCH,1flowbase-egress"]
    });
    let yaml = serde_yaml::to_string(&payload).context("cannot render Mihomo lease config")?;
    fs::write(&config_path, yaml).context("cannot write Mihomo lease config")?;
    Ok((config_path, directory))
}

fn wait_for_loopback(address: &str, child: &mut Child) -> Result<()> {
    let socket: SocketAddr = address
        .parse()
        .context("invalid loopback listener address")?;
    let deadline = std::time::Instant::now() + CORE_READY_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            bail!("bundled Mihomo core exited before binding its loopback listener");
        }
        if TcpStream::connect_timeout(&socket, Duration::from_millis(100)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    bail!("bundled Mihomo core did not bind its loopback listener")
}

fn random_token() -> String {
    let mut bytes = [0_u8; 24];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn run_stdio(config_path: &Path) -> Result<()> {
    let mut worker = Worker::start(config_path)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in BufReader::new(stdin.lock()).lines() {
        let request = line
            .context("cannot read worker request")
            .and_then(|line| serde_json::from_str::<Value>(&line).context("invalid JSON request"));
        let response = match request {
            Ok(request) => {
                let operation = request
                    .get("operation")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                worker
                    .handle(request)
                    .unwrap_or_else(|error| safe_error_response(operation.as_deref(), &error))
            }
            Err(error) => safe_error_response(None, &error),
        };
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

fn safe_error_response(operation: Option<&str>, error: &anyhow::Error) -> Value {
    let safe_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<EgressError>());
    let (code, message) = safe_error
        .map(|error| (error.code, error.message))
        .unwrap_or((
            SUBSCRIPTION_UNAVAILABLE_CODE,
            "Network egress is unavailable.",
        ));
    let mut response = json!({"error": {"code": code, "message": message}});
    if let Some(operation) = operation {
        response["operation"] = Value::String(operation.to_owned());
    }
    response
}

pub fn contract_version() -> &'static str {
    CONTRACT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/representative-v1.yaml");
    const REALISTIC_SUBSCRIPTION: &str =
        include_str!("../tests/fixtures/realistic-clash-subscription.yaml");

    #[test]
    fn nc_06_fixed_yaml_v1_projects_stable_ss_vmess_vless_and_trojan_egresses() {
        let config =
            FixedYamlV1::from_yaml(FIXTURE).expect("representative fixed YAML is accepted");
        let keys = config
            .egresses()
            .iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 4);
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(config
            .egresses()
            .iter()
            .any(|egress| egress.proxy.kind == "vmess"));
        assert!(config
            .egresses()
            .iter()
            .all(|egress| egress.key.starts_with("clash/") && egress.key.is_ascii()));
        assert_eq!(contract_version(), "1flowbase.network_egress_provider/v1");
    }

    #[test]
    fn nc_07_rejects_uris_base64_provider_only_and_non_remote_proxy_types() {
        for invalid in include_str!("../tests/fixtures/unsupported-inputs.txt").split("\n---\n") {
            assert!(FixedYamlV1::from_yaml(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn nc_07_projects_realistic_clash_subscription_nodes_without_its_global_config() {
        let config = FixedYamlV1::from_yaml(REALISTIC_SUBSCRIPTION)
            .expect("realistic Clash subscription is accepted");
        let keys = config
            .egresses()
            .iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 3);
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(config
            .egresses()
            .iter()
            .any(|egress| egress.proxy.kind == "hysteria2"));
    }

    #[test]
    fn nc_07_preserves_supported_node_options_in_the_isolated_mihomo_config() {
        let proxy = FixedYamlV1::from_yaml(REALISTIC_SUBSCRIPTION)
            .unwrap()
            .egresses()
            .iter()
            .find(|egress| egress.proxy.kind == "vless")
            .expect("fixture includes VLESS")
            .proxy
            .clone();
        let (path, directory) = write_core_config(&proxy, "127.0.0.1:18080").unwrap();
        let config = fs::read_to_string(&path).unwrap();
        assert!(config.contains("reality-opts"));
        assert!(config.contains("public-key"));
        assert!(!config.contains("AUTO"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn nc_07_uses_a_clash_compatible_subscription_user_agent() {
        assert_eq!(SUBSCRIPTION_USER_AGENT, "clash.meta");
    }

    #[test]
    fn nc_09_subscription_fetch_does_not_inherit_the_host_ambient_proxy() {
        assert!(subscription_agent().config().proxy().is_none());
    }

    #[test]
    fn qf_003_subscription_configuration_requires_https_before_any_network_access() {
        let path = std::env::temp_dir().join(format!("clash-proxy-secret-{}.json", now_millis()));
        fs::write(&path, r#"{"subscription_url":"http://127.0.0.1/private"}"#).unwrap();
        let result = FixedYamlV1::from_secret_file(&path);
        let _ = fs::remove_file(&path);
        let error = result.unwrap_err();
        assert_eq!(
            safe_error_response(None, &error)["error"]["code"],
            SUBSCRIPTION_INVALID_CODE
        );
    }

    #[test]
    fn nc_06_rejects_secret_or_config_on_the_public_stdio_abi() {
        let config = FixedYamlV1::from_yaml(FIXTURE).unwrap();
        let mut worker = Worker::new(config, PathBuf::from("missing-core")).unwrap();
        assert!(worker
            .handle(json!({"operation":"sync_egresses","input":{"secret":"no"}}))
            .is_err());
        assert!(worker.handle(json!({"operation":"acquire_http_forward_proxy","input":{"provider_egress_key":"clash/ss-us","provider_config":{}}})).is_err());
    }

    #[test]
    fn nc_06_generates_loopback_only_mihomo_config_without_tun_or_system_proxy() {
        let proxy = FixedYamlV1::from_yaml(FIXTURE)
            .unwrap()
            .egresses()
            .iter()
            .find(|egress| egress.proxy.kind == "ss")
            .expect("fixture includes SS")
            .proxy
            .clone();
        let (path, directory) = write_core_config(&proxy, "127.0.0.1:18080").unwrap();
        let config = fs::read_to_string(&path).unwrap();
        assert!(config.contains("bind-address: 127.0.0.1"));
        assert!(config.contains("mixed-port: 18080"));
        assert!(!config.contains("tun:"));
        assert!(!config.contains("system-proxy"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ac_006_worker_drop_terminates_runtime_closes_port_and_removes_config() {
        let directory =
            std::env::temp_dir().join(format!("clash-proxy-cleanup-{}", random_token()));
        fs::create_dir(&directory).unwrap();
        let config_path = directory.join("mihomo.yaml");
        let stderr_path = directory.join("mihomo.stderr");
        fs::write(&config_path, "mixed-port: 1\n").unwrap();
        fs::write(&stderr_path, "").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let child = Command::new("python3")
            .args([
                "-c",
                "import socket,sys,time; s=socket.socket(); s.bind(('127.0.0.1',int(sys.argv[1]))); s.listen(); time.sleep(30)",
                &address.port().to_string(),
            ])
            .spawn()
            .unwrap();
        let child_id = child.id();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while TcpStream::connect(address).is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "test core did not listen"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let runtime = MihomoRuntime {
            proxy_url: format!("http://{address}"),
            config_path: config_path.clone(),
            stderr_path: stderr_path.clone(),
            config_dir: directory.clone(),
            child,
        };
        let pool = Arc::new(Mutex::new(RuntimePool::new(1, 60_000, 5_000)));
        pool.lock()
            .unwrap()
            .acquire(
                "lifecycle",
                "lease-lifecycle".into(),
                "token-lifecycle".into(),
                now_millis(),
                || Ok(runtime),
            )
            .unwrap();
        let worker = Worker {
            config: FixedYamlV1::from_yaml(FIXTURE).unwrap(),
            core_path: PathBuf::from("unused-test-core"),
            pool,
            reaper_stop: None,
            reaper: None,
        };
        drop(worker);

        assert!(!config_path.exists());
        assert!(!stderr_path.exists());
        assert!(!directory.exists());
        assert!(TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_err());
        assert!(!PathBuf::from(format!("/proc/{child_id}")).exists());
    }

    #[test]
    fn ac_008_classifies_startup_stages_and_never_exposes_internal_causes() {
        let private_path = "/private/subscription/token/mihomo";
        let error = runtime_start_failed_error(
            "Proxy runtime process could not be created.",
            std::io::Error::new(std::io::ErrorKind::NotFound, private_path),
        );
        let response = safe_error_response(Some("acquire_http_forward_proxy"), &error);
        assert_eq!(response["error"]["code"], RUNTIME_START_FAILED_CODE);
        assert_eq!(
            response["error"]["message"],
            "Proxy runtime process could not be created."
        );
        assert!(!response.to_string().contains(private_path));

        let directory = std::env::temp_dir().join(format!(
            "clash-proxy-resource-diagnostic-{}",
            random_token()
        ));
        fs::create_dir(&directory).unwrap();
        let stderr_path = directory.join("mihomo.stderr");
        fs::write(
            &stderr_path,
            "runtime: failed to reserve page summary memory\nprivate-token",
        )
        .unwrap();
        assert!(stderr_reports_resource_exhaustion(&stderr_path));
        let error = runtime_resource_exhausted_error(anyhow!("private-token"));
        let response = safe_error_response(Some("acquire_http_forward_proxy"), &error);
        assert_eq!(response["error"]["code"], RUNTIME_RESOURCE_EXHAUSTED_CODE);
        assert!(!response.to_string().contains("private-token"));
        fs::remove_file(stderr_path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "source integration: requires ONEFLOWBASE_TEST_MIHOMO_CORE and Linux procfs"]
    fn ac_003_mihomo_resource_benchmark_under_one_gib_address_space() {
        use std::os::unix::process::CommandExt;

        const ADDRESS_SPACE_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;
        const POOL_RSS_BUDGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
        const WORKER_RSS_ALLOWANCE_BYTES: u64 = 256 * 1024 * 1024;
        const RUNTIME_PEAK_RSS_ALLOWANCE_BYTES: u64 = 384 * 1024 * 1024;

        let core_path = std::env::var_os("ONEFLOWBASE_TEST_MIHOMO_CORE")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("ONEFLOWBASE_TEST_MIHOMO_CORE must point to a real Mihomo executable");
        let proxy = FixedYamlV1::from_yaml(FIXTURE).unwrap().egresses()[0]
            .proxy
            .clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        drop(listener);
        let (config_path, config_dir) = write_core_config(&proxy, &address).unwrap();
        let stderr_path = config_dir.join("mihomo.stderr");
        let stderr = fs::File::create(&stderr_path).unwrap();
        let mut command = Command::new(core_path);
        command
            .arg("-f")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: ADDRESS_SPACE_LIMIT_BYTES,
                    rlim_max: ADDRESS_SPACE_LIMIT_BYTES,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        let mut child = command.spawn().unwrap();
        if let Err(error) = wait_for_loopback(&address, &mut child) {
            let _ = child.kill();
            let _ = child.wait();
            let resource_exhausted = stderr_reports_resource_exhaustion(&stderr_path);
            cleanup_runtime_files(&config_path, &stderr_path, &config_dir);
            panic!(
                "real Mihomo failed under 1 GiB RLIMIT_AS: {error}; resource_exhausted={resource_exhausted}"
            );
        }

        let mut stable_rss_kib = 0_u64;
        let mut peak_rss_kib = 0_u64;
        for _ in 0..20 {
            let status = fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap();
            stable_rss_kib = proc_status_kib(&status, "VmRSS:").unwrap_or(stable_rss_kib);
            peak_rss_kib =
                peak_rss_kib.max(proc_status_kib(&status, "VmHWM:").unwrap_or(stable_rss_kib));
            thread::sleep(Duration::from_millis(25));
        }
        let peak_rss_bytes = peak_rss_kib * 1024;
        let derived_capacity =
            (POOL_RSS_BUDGET_BYTES - WORKER_RSS_ALLOWANCE_BYTES) / RUNTIME_PEAK_RSS_ALLOWANCE_BYTES;
        eprintln!(
            "mihomo_resource_evidence stable_rss_kib={stable_rss_kib} peak_rss_kib={peak_rss_kib} address_space_limit_bytes={ADDRESS_SPACE_LIMIT_BYTES} pool_rss_budget_bytes={POOL_RSS_BUDGET_BYTES} worker_rss_allowance_bytes={WORKER_RSS_ALLOWANCE_BYTES} runtime_peak_rss_allowance_bytes={RUNTIME_PEAK_RSS_ALLOWANCE_BYTES} derived_capacity={derived_capacity}"
        );
        assert!(peak_rss_bytes > 0);
        assert!(peak_rss_bytes <= RUNTIME_PEAK_RSS_ALLOWANCE_BYTES);
        assert_eq!(derived_capacity as usize, MAX_ACTIVE_RUNTIMES);
        assert!(TcpStream::connect(&address).is_ok());

        let _ = child.kill();
        let _ = child.wait();
        cleanup_runtime_files(&config_path, &stderr_path, &config_dir);
    }

    #[cfg(target_os = "linux")]
    fn proc_status_kib(status: &str, field: &str) -> Option<u64> {
        status.lines().find_map(|line| {
            line.strip_prefix(field)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
    }

    #[test]
    fn nc_08_accepts_unicode_display_names_and_uses_stable_ascii_configuration_keys() {
        let first = r#"
proxies:
  - name: "东京 🗼"
    type: ss
    server: first.example.test
    port: 8388
    cipher: aes-128-gcm
    password: redacted-first
  - name: "东京 🗼"
    type: ss
    server: second.example.test
    port: 8388
    cipher: aes-128-gcm
    password: redacted-second
"#;
        let reordered = r#"
proxies:
  - name: "东京 🗼"
    type: ss
    server: second.example.test
    port: 8388
    cipher: aes-128-gcm
    password: redacted-second
  - name: "东京 🗼"
    type: ss
    server: first.example.test
    port: 8388
    cipher: aes-128-gcm
    password: redacted-first
"#;

        let first = FixedYamlV1::from_yaml(first).expect("Unicode display names are accepted");
        let reordered =
            FixedYamlV1::from_yaml(reordered).expect("subscription node order does not matter");
        let first_keys = first
            .egresses()
            .iter()
            .map(|egress| egress.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(first_keys.len(), 2);
        assert_ne!(first_keys[0], first_keys[1]);
        assert!(first_keys
            .iter()
            .all(|key| key.starts_with("clash/") && key.is_ascii()));
        assert!(first
            .egresses()
            .iter()
            .all(|egress| egress.display_name == "东京 🗼"));
        assert_eq!(
            first_keys,
            reordered
                .egresses()
                .iter()
                .map(|egress| egress.key.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn nc_08_rejects_duplicate_complete_proxy_nodes() {
        let duplicate = r#"
proxies:
  - name: "东京 🗼"
    type: ss
    server: duplicate.example.test
    port: 8388
    cipher: aes-128-gcm
    password: redacted
  - password: redacted
    cipher: aes-128-gcm
    port: 8388
    server: duplicate.example.test
    type: ss
    name: "东京 🗼"
"#;

        let error = FixedYamlV1::from_yaml(duplicate).expect_err("duplicate node is rejected");
        assert_eq!(
            safe_error_response(Some("sync_egresses"), &error)["error"]["code"],
            PROXY_INVALID_CODE
        );
    }

    #[test]
    fn nc_08_emits_only_safe_parse_error_codes_and_messages() {
        let invalid_subscription =
            FixedYamlV1::from_yaml("https://private.example.test/token").unwrap_err();
        let invalid_proxy = FixedYamlV1::from_yaml(
            "proxies:\n  - name: private-node\n    type: ss\n    server: example.test\n    port: 0\n",
        )
        .unwrap_err();

        for (error, code, secret) in [
            (
                invalid_subscription,
                SUBSCRIPTION_INVALID_CODE,
                "https://private.example.test/token",
            ),
            (invalid_proxy, PROXY_INVALID_CODE, "private-node"),
        ] {
            let response = safe_error_response(Some("sync_egresses"), &error);
            assert_eq!(response["operation"], "sync_egresses");
            assert_eq!(response["error"]["code"], code);
            let message = response["error"]["message"].as_str().unwrap();
            assert!(message.len() <= 64);
            assert!(!message.contains(secret));
        }
    }

    #[test]
    fn ac_003_ac_005_emits_stable_runtime_capacity_and_backoff_codes() {
        for (pool_error, expected_code) in [
            (
                PoolError::CapacityExhausted,
                RUNTIME_CAPACITY_EXHAUSTED_CODE,
            ),
            (PoolError::Backoff, RUNTIME_BACKOFF_CODE),
        ] {
            let error = map_runtime_pool_error(pool_error.into());
            let response = safe_error_response(Some("acquire_http_forward_proxy"), &error);
            assert_eq!(response["error"]["code"], expected_code);
            assert_eq!(response["operation"], "acquire_http_forward_proxy");
        }
    }
}
