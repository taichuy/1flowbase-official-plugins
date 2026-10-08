#!/usr/bin/env bash
# Public installer. Application credentials are read locally, never from a URL.
set -euo pipefail
version=0.2.0
endpoint=''
source_path="${CODEX_HOME:-$HOME/.codex}"
installation_id=default
install_dir=''
config_path=''
release_base=''
start_service=1
uninstall=0
usage() {
  cat <<'EOF'
Codex native logs collector installer
  --endpoint URL             Exact 1flowbase /api/logs/v1/events endpoint
  --installation-id ID       Application-specific installation (default: default)
  --source PATH              Codex home, selected directory or rollout file
  --version VERSION          Immutable collector release (default: 0.2.0)
  --install-dir PATH         Override the user installation directory
  --config PATH              Override the private configuration path
  --release-base URL         Required 1flowbase version-pinned asset base URL
  --no-start                 Configure without registering a background service
  --uninstall                Stop/remove collector; retain config and checkpoint
Credentials: local terminal prompt, or FLOWBASE_AGENT_LOGS_API_KEY environment.
No sudo, Node.js or Rust toolchain is required.
EOF
}
while (($#)); do
  case "$1" in
    --help|-h) usage; exit 0 ;;
    --no-start) start_service=0; shift ;;
    --uninstall) uninstall=1; shift ;;
    --endpoint|--source|--installation-id|--version|--install-dir|--config|--release-base)
      (($# >= 2)) || { printf 'Missing option value\n' >&2; exit 2; }
      case "$1" in
        --endpoint) endpoint=$2 ;; --source) source_path=$2 ;;
        --installation-id) installation_id=$2 ;; --version) version=$2 ;;
        --install-dir) install_dir=$2 ;; --config) config_path=$2 ;;
        --release-base) release_base=$2 ;;
      esac
      shift 2 ;;
    *) printf 'Unknown installer option; use --help\n' >&2; exit 2 ;;
  esac
done
[[ "$installation_id" =~ ^[a-zA-Z0-9._-]+$ ]] || { printf 'Invalid installation ID\n' >&2; exit 2; }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { printf 'Invalid release version\n' >&2; exit 2; }
install_dir=${install_dir:-"${XDG_DATA_HOME:-$HOME/.local/share}/1flowbase/collectors/codex/$installation_id"}
config_path=${config_path:-"$install_dir/config.json"}
service_name="1flowbase-codex-logs-$installation_id"
binary_path="$install_dir/bin/codex-logs-collector"
os=$(uname -s)
case "$os" in Linux) os=linux ;; Darwin) os=darwin ;; *) printf 'Unsupported OS; use the Windows PowerShell installer\n' >&2; exit 2 ;; esac
case "$(uname -m)" in x86_64|amd64) arch=amd64 ;; aarch64|arm64) arch=arm64 ;; *) printf 'Unsupported architecture\n' >&2; exit 2 ;; esac
for value in "$install_dir" "$config_path" "$source_path"; do
  [[ "$value" != *$'\n'* && "$value" != *$'\r'* ]] || { printf 'Paths must not contain line breaks\n' >&2; exit 2; }
done
unit_path="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$service_name.service"
launch_path="$HOME/Library/LaunchAgents/$service_name.plist"
if ((uninstall)); then
  if [[ "$os" == linux ]]; then
    if [[ -f "$unit_path" ]]; then
      systemctl --user disable --now "$service_name.service"
      rm -f "$unit_path"
      systemctl --user daemon-reload
    fi
  elif [[ -f "$launch_path" ]]; then
    launchctl bootout "gui/$(id -u)" "$launch_path" 2>/dev/null || true
    rm -f "$launch_path"
  fi
  rm -f "$binary_path"
  printf 'Collector removed. Configuration, checkpoint and source logs retained.\n'
  exit 0
fi
[[ -n "$endpoint" ]] || { printf '%s\n' '--endpoint is required' >&2; exit 2; }
[[ -n "$release_base" ]] || { printf '%s\n' '--release-base is required (copy the command from 1flowbase)' >&2; exit 2; }
release_base=${release_base%/}
[[ "$release_base" == https://* || "$release_base" == http://* ]] && [[ "$release_base" != *@* && "$release_base" != *'?'* && "$release_base" != *'#'* ]] || { printf 'Invalid public release URL\n' >&2; exit 2; }
umask 077
staging=$(mktemp -d)
restore_service=0
cleanup() {
  unset collector_key
  if ((restore_service)); then
    if [[ "$os" == linux ]]; then systemctl --user start "$service_name.service" >&2 || true
    else launchctl bootstrap "gui/$(id -u)" "$launch_path" >&2 || true; fi
  fi
  rm -rf "$staging"
}
trap cleanup EXIT
archive="codex-logs-collector-$version-$os-$arch.tar.gz"
download_asset() {
  local status
  status=$(curl --fail --silent --show-error --proto '=http,https' --write-out '%{http_code}' "$release_base/$1" -o "$staging/$1")
  [[ "$status" == 200 ]] || { printf 'Asset download requires HTTP 200; redirects are refused\n' >&2; return 1; }
}
download_asset "$archive"
download_asset checksums.txt
expected=$(awk -v file="$archive" '$2 == file { print $1 }' "$staging/checksums.txt")
[[ "$expected" =~ ^[a-fA-F0-9]{64}$ ]] || { printf 'Release checksum missing or invalid\n' >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$staging/$archive"); actual=${actual%% *}
else
  actual=$(shasum -a 256 "$staging/$archive"); actual=${actual%% *}
fi
[[ "$actual" == "$expected" ]] || { printf 'Release checksum mismatch; existing installation retained\n' >&2; exit 1; }
# Extract only the executable selected by this installer.
tar -xzf "$staging/$archive" -C "$staging" ./codex-logs-collector
chmod 700 "$staging/codex-logs-collector"
if [[ "$("$staging/codex-logs-collector" --version)" != "codex-logs-collector $version" ]]; then
  printf 'Executable version differs from selected release\n' >&2; exit 1
fi
mkdir -p "$install_dir/bin" "$(dirname "$config_path")"
chmod 700 "$install_dir" "$install_dir/bin"
if [[ -n "${FLOWBASE_AGENT_LOGS_API_KEY:-}" ]]; then
  collector_key=$FLOWBASE_AGENT_LOGS_API_KEY
elif [[ -r /dev/tty && -w /dev/tty ]]; then
  printf 'Application API Key (saved only in local private config): ' >/dev/tty
  IFS= read -r -s collector_key </dev/tty
  printf '\n' >/dev/tty
else
  printf 'No interactive terminal. Set FLOWBASE_AGENT_LOGS_API_KEY and run again.\n' >&2; exit 1
fi
[[ -n "$collector_key" ]] || { printf 'API Key must not be empty\n' >&2; exit 1; }
# Configuration and collection share the checkpoint lock. Stop this installation
# before rotating its key, and restore the existing service if configuration fails.
if [[ "$os" == linux && -f "$unit_path" ]] && systemctl --user is-active --quiet "$service_name.service"; then
  systemctl --user stop "$service_name.service"
  restore_service=1
elif [[ "$os" == darwin && -f "$launch_path" ]]; then
  if launchctl bootout "gui/$(id -u)" "$launch_path" 2>/dev/null; then restore_service=1; fi
fi
printf '%s\n' "$collector_key" | "$staging/codex-logs-collector" configure --endpoint "$endpoint" --source "$source_path" --config "$config_path" --key-stdin
unset collector_key
cp "$staging/codex-logs-collector" "$install_dir/bin/.codex-logs-collector.new"
chmod 700 "$install_dir/bin/.codex-logs-collector.new"
mv -f "$install_dir/bin/.codex-logs-collector.new" "$binary_path"
if ((!start_service)); then
  restore_service=0
  if [[ "$os" == linux && -f "$unit_path" ]]; then systemctl --user disable "$service_name.service"; fi
  if [[ "$os" == darwin ]]; then rm -f "$launch_path"; fi
  printf 'Collector configured. Background startup was explicitly disabled.\n'
  exit 0
fi
if [[ "$os" == linux ]]; then
  command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1 || {
    printf 'Installed, but no user systemd session is available. Start the native collector with watch --config, or rerun in a supported user session.\n' >&2; exit 1;
  }
  escape_systemd() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/%/%%/g; s/\$/$$/g'; }
  mkdir -p "$(dirname "$unit_path")"
  cat >"$unit_path" <<EOF
[Unit]
Description=1flowbase Codex logs collector ($installation_id)
[Service]
ExecStart="$(escape_systemd "$binary_path")" watch --config "$(escape_systemd "$config_path")"
Restart=on-failure
RestartSec=5
[Install]
WantedBy=default.target
EOF
  systemctl --user daemon-reload
  systemctl --user enable "$service_name.service"
  systemctl --user restart "$service_name.service"
  systemctl --user is-active --quiet "$service_name.service"
else
  escape_xml() { printf '%s' "$1" | sed 's/\&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g; s/"/\&quot;/g'; }
  mkdir -p "$(dirname "$launch_path")"
  cat >"$launch_path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>$(escape_xml "$service_name")</string>
<key>ProgramArguments</key><array><string>$(escape_xml "$binary_path")</string><string>watch</string><string>--config</string><string>$(escape_xml "$config_path")</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>$(escape_xml "$install_dir/collector.log")</string>
<key>StandardErrorPath</key><string>$(escape_xml "$install_dir/collector-error.log")</string>
</dict></plist>
EOF
  launchctl bootout "gui/$(id -u)" "$launch_path" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$launch_path"
  launchctl kickstart "gui/$(id -u)/$service_name"
fi
restore_service=0
printf 'Collector installed and started. It resumes after this user logs in. Source logs are read only.\n'
