use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const CONTRACT_VERSION: &str = "1flowbase.network_egress_provider/v1";
const LEASE_DURATION_MILLIS: u64 = 300_000;
const CORE_READY_TIMEOUT: Duration = Duration::from_secs(5);
static NEXT_LEASE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub server: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cipher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(rename = "alterId", skip_serializing_if = "Option::is_none")]
    pub alter_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Egress {
    pub key: String,
    pub display_name: String,
    pub proxy: ProxyEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedYamlV1 {
    egresses: Vec<Egress>,
}

impl FixedYamlV1 {
    /// The host's startup-only secret carrier is JSON. The subscription URL is read only by the
    /// provider, fetched over HTTPS, and constrained to the fixed Clash/Mihomo YAML subset.
    pub fn from_secret_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).context("cannot read private network egress config")?;
        let secret = serde_json::from_str::<serde_json::Value>(&raw)
            .context("network egress secret must be a JSON object")?;
        let subscription_url = secret
            .get("subscription_url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| value.starts_with("https://") && value.len() <= 2048)
            .ok_or_else(|| anyhow!("subscription_url must be an HTTPS URL"))?;
        let mut response = ureq::get(subscription_url)
            .config()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .call()
            .context("cannot fetch Clash/Mihomo subscription")?;
        let yaml = response
            .body_mut()
            .read_to_string()
            .context("cannot read Clash/Mihomo subscription")?;
        if yaml.len() > 1_048_576 {
            bail!("Clash/Mihomo subscription exceeds 1 MiB");
        }
        Self::from_yaml(&yaml)
    }

    pub fn from_yaml(raw: &str) -> Result<Self> {
        if raw.trim().is_empty() || looks_like_base64(raw) {
            bail!("only a fixed Clash/Mihomo YAML v1 document is accepted");
        }
        let root: serde_yaml::Mapping =
            serde_yaml::from_str(raw).context("network egress secret must be a YAML mapping")?;
        if root.len() != 1 || !root.contains_key(serde_yaml::Value::String("proxies".to_owned())) {
            bail!("fixed Clash/Mihomo YAML v1 permits only the top-level proxies field");
        }
        let proxies = root
            .get(serde_yaml::Value::String("proxies".to_owned()))
            .and_then(serde_yaml::Value::as_sequence)
            .ok_or_else(|| anyhow!("proxies must be a YAML sequence"))?;
        if proxies.is_empty() {
            bail!("proxies must not be empty");
        }

        let mut egresses = Vec::with_capacity(proxies.len());
        for value in proxies {
            let proxy = parse_proxy(value)?;
            let key = format!("clash/{}", proxy.name);
            egresses.push(Egress {
                key,
                display_name: proxy.name.clone(),
                proxy,
            });
        }
        egresses.sort_by(|left, right| left.key.cmp(&right.key));
        if egresses.windows(2).any(|pair| pair[0].key == pair[1].key) {
            bail!("proxy names must be unique");
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

fn parse_proxy(value: &serde_yaml::Value) -> Result<ProxyEntry> {
    let object = value
        .as_mapping()
        .ok_or_else(|| anyhow!("each proxy must be a YAML mapping"))?;
    let mut fields = BTreeMap::new();
    for (key, value) in object {
        let key = key
            .as_str()
            .ok_or_else(|| anyhow!("proxy field names must be strings"))?;
        fields.insert(key, value);
    }
    let name = required_safe_text(&fields, "name")?;
    let kind = required_safe_text(&fields, "type")?.to_ascii_lowercase();
    let server = required_safe_text(&fields, "server")?;
    if server.contains("://") || server.contains('@') || server.contains('/') {
        bail!("server must be a hostname or IP address, not a URI");
    }
    let port = fields
        .get("port")
        .and_then(|value| value.as_u64())
        .filter(|port| (1..=65535).contains(port))
        .map(|port| port as u16)
        .ok_or_else(|| anyhow!("port must be an integer between 1 and 65535"))?;

    let allowed = match kind.as_str() {
        "ss" => ["name", "type", "server", "port", "cipher", "password"].as_slice(),
        "vmess" => [
            "name", "type", "server", "port", "uuid", "alterId", "cipher",
        ]
        .as_slice(),
        "vless" => ["name", "type", "server", "port", "uuid", "flow"].as_slice(),
        "trojan" => ["name", "type", "server", "port", "password"].as_slice(),
        _ => bail!("supported proxy types are ss, vmess, vless, and trojan"),
    };
    if fields.keys().any(|field| !allowed.contains(field)) {
        bail!("fixed Clash/Mihomo YAML v1 rejects unsupported proxy fields");
    }

    let cipher = optional_safe_text(&fields, "cipher")?;
    let password = optional_safe_text(&fields, "password")?;
    let uuid = optional_safe_text(&fields, "uuid")?;
    let alter_id = fields
        .get("alterId")
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value <= u16::MAX as u64)
                .map(|value| value as u16)
                .ok_or_else(|| anyhow!("alterId must be a non-negative integer"))
        })
        .transpose()?;
    let flow = optional_safe_text(&fields, "flow")?;

    match kind.as_str() {
        "ss" if cipher.is_none() || password.is_none() => {
            bail!("ss requires cipher and password")
        }
        "vmess" if uuid.is_none() => bail!("vmess requires uuid"),
        "vless" if uuid.is_none() => bail!("vless requires uuid"),
        "trojan" if password.is_none() => bail!("trojan requires password"),
        _ => {}
    }
    Ok(ProxyEntry {
        name,
        kind,
        server,
        port,
        cipher,
        password,
        uuid,
        alter_id,
        flow,
    })
}

fn required_safe_text(fields: &BTreeMap<&str, &serde_yaml::Value>, field: &str) -> Result<String> {
    optional_safe_text(fields, field)?.ok_or_else(|| anyhow!("{field} must be a non-empty string"))
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
        .ok_or_else(|| anyhow!("{field} must be a non-empty string"))?;
    if value.len() > 256 || value.contains('\n') || value.contains('\r') {
        bail!("{field} has an invalid value");
    }
    if field == "name"
        && (!value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ' ')
        }) || value.starts_with([' ', '.', '-']))
    {
        bail!("name must be a safe, non-sensitive display identifier");
    }
    Ok(Some(value.to_owned()))
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
struct Lease {
    cleanup_token: String,
    config_path: PathBuf,
    config_dir: PathBuf,
    child: Child,
}

impl Lease {
    fn terminate(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.config_path);
        let _ = fs::remove_dir(&self.config_dir);
    }
}

pub struct Worker {
    config: FixedYamlV1,
    core_path: PathBuf,
    leases: HashMap<String, Lease>,
}

impl Worker {
    pub fn start(config_path: &Path) -> Result<Self> {
        Ok(Self {
            config: FixedYamlV1::from_secret_file(config_path)?,
            core_path: bundled_core_path()?,
            leases: HashMap::new(),
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
        let listener =
            TcpListener::bind("127.0.0.1:0").context("cannot reserve loopback proxy port")?;
        let address = listener.local_addr()?.to_string();
        drop(listener);
        let (config_path, config_dir) = write_core_config(&egress.proxy, &address)?;
        let mut child = Command::new(&self.core_path)
            .arg("-f")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| {
                format!(
                    "cannot start bundled Mihomo core at {}",
                    self.core_path.display()
                )
            })?;
        if let Err(error) = wait_for_loopback(&address) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&config_path);
            let _ = fs::remove_dir(&config_dir);
            return Err(error);
        }
        let sequence = NEXT_LEASE.fetch_add(1, Ordering::Relaxed);
        let lease_id = format!("clash-{}-{sequence}", now_millis());
        let cleanup_token = random_token();
        let proxy_url = format!("http://{address}");
        self.leases.insert(
            lease_id.clone(),
            Lease {
                cleanup_token: cleanup_token.clone(),
                config_path,
                config_dir,
                child,
            },
        );
        Ok((lease_id, proxy_url, cleanup_token))
    }

    fn release(&mut self, lease_id: &str, cleanup_token: &str) -> Result<()> {
        let lease = self
            .leases
            .remove(lease_id)
            .ok_or_else(|| anyhow!("unknown lease_id"))?;
        if lease.cleanup_token != cleanup_token {
            self.leases.insert(lease_id.to_owned(), lease);
            bail!("cleanup_token does not match lease_id");
        }
        lease.terminate();
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        for (_, lease) in std::mem::take(&mut self.leases) {
            lease.terminate();
        }
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
        "proxies": [proxy],
        "proxy-groups": [{"name":"1flowbase-egress", "type":"select", "proxies":[proxy.name]}],
        "rules": ["MATCH,1flowbase-egress"]
    });
    let yaml = serde_yaml::to_string(&payload).context("cannot render Mihomo lease config")?;
    fs::write(&config_path, yaml).context("cannot write Mihomo lease config")?;
    Ok((config_path, directory))
}

fn wait_for_loopback(address: &str) -> Result<()> {
    let socket: SocketAddr = address
        .parse()
        .context("invalid loopback listener address")?;
    let deadline = std::time::Instant::now() + CORE_READY_TIMEOUT;
    while std::time::Instant::now() < deadline {
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
        let response = line
            .context("cannot read worker request")
            .and_then(|line| serde_json::from_str::<Value>(&line).context("invalid JSON request"))
            .and_then(|request| worker.handle(request))
            .unwrap_or_else(|error| json!({"error": {"code":"network_egress_invalid_request", "message": error.to_string()}}));
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

pub fn contract_version() -> &'static str {
    CONTRACT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/representative-v1.yaml");

    #[test]
    fn nc_06_fixed_yaml_v1_projects_stable_ss_vmess_vless_and_trojan_egresses() {
        let config =
            FixedYamlV1::from_yaml(FIXTURE).expect("representative fixed YAML is accepted");
        assert_eq!(
            config
                .egresses()
                .iter()
                .map(|item| item.key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "clash/ss-us",
                "clash/trojan-ca",
                "clash/vless-ap",
                "clash/vmess-eu"
            ]
        );
        assert_eq!(config.egress("clash/vmess-eu").unwrap().proxy.kind, "vmess");
        assert_eq!(contract_version(), "1flowbase.network_egress_provider/v1");
    }

    #[test]
    fn nc_06_rejects_uris_base64_providers_and_non_v1_proxy_fields() {
        for invalid in include_str!("../tests/fixtures/unsupported-inputs.txt").split("\n---\n") {
            assert!(FixedYamlV1::from_yaml(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn qf_003_subscription_configuration_requires_https_before_any_network_access() {
        let path = std::env::temp_dir().join(format!("clash-proxy-secret-{}.json", now_millis()));
        fs::write(&path, r#"{"subscription_url":"http://127.0.0.1/private"}"#).unwrap();
        let result = FixedYamlV1::from_secret_file(&path);
        let _ = fs::remove_file(&path);
        assert!(result.unwrap_err().to_string().contains("HTTPS URL"));
    }

    #[test]
    fn nc_06_rejects_secret_or_config_on_the_public_stdio_abi() {
        let config = FixedYamlV1::from_yaml(FIXTURE).unwrap();
        let mut worker = Worker {
            config,
            core_path: PathBuf::from("missing-core"),
            leases: HashMap::new(),
        };
        assert!(worker
            .handle(json!({"operation":"sync_egresses","input":{"secret":"no"}}))
            .is_err());
        assert!(worker.handle(json!({"operation":"acquire_http_forward_proxy","input":{"provider_egress_key":"clash/ss-us","provider_config":{}}})).is_err());
    }

    #[test]
    fn nc_06_generates_loopback_only_mihomo_config_without_tun_or_system_proxy() {
        let proxy = FixedYamlV1::from_yaml(FIXTURE)
            .unwrap()
            .egress("clash/ss-us")
            .unwrap()
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
    fn nc_06_release_terminates_the_core_and_removes_the_ephemeral_config() {
        let directory =
            std::env::temp_dir().join(format!("clash-proxy-cleanup-{}", random_token()));
        fs::create_dir(&directory).unwrap();
        let config_path = directory.join("mihomo.yaml");
        fs::write(&config_path, "mixed-port: 1\n").unwrap();
        let child = Command::new("sh").args(["-c", "sleep 30"]).spawn().unwrap();
        let lease = Lease {
            cleanup_token: "opaque".to_owned(),
            config_path: config_path.clone(),
            config_dir: directory.clone(),
            child,
        };
        lease.terminate();
        assert!(!config_path.exists());
        assert!(!directory.exists());
    }
}
