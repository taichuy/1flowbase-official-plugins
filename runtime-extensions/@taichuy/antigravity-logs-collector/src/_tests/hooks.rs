use super::*;
use agent_logs_collector::configure;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agy-hooks-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn register_installs_isolated_plugin_preserves_multi_app_and_cleans_up_on_uninstall() {
    let dir = temp_dir("register");
    let gemini = dir.join("gemini");
    let global_config = gemini.join("config/config.json");
    let global_hooks = gemini.join("config/hooks.json");
    fs::create_dir_all(global_config.parent().unwrap()).unwrap();
    fs::write(&global_config, "{\"plugins\":{}}\n").unwrap();
    fs::write(&global_hooks, "{\"existing\":{}}\n").unwrap();

    let cfg_a = dir.join("app-a/config.json");
    let cfg_b = dir.join("app-b/config.json");
    configure(
        &cfg_a,
        "https://example.invalid/api/logs/v1/events".into(),
        &gemini,
        "k1".into(),
    )
    .unwrap();
    configure(
        &cfg_b,
        "https://example.invalid/api/logs/v1/events".into(),
        &gemini,
        "k2".into(),
    )
    .unwrap();

    register(&cfg_a, &gemini, true).unwrap();
    register(&cfg_b, &gemini, true).unwrap();

    // Global config.json and hooks.json must remain untouched byte-for-byte
    assert_eq!(
        fs::read_to_string(&global_config).unwrap(),
        "{\"plugins\":{}}\n"
    );
    assert_eq!(
        fs::read_to_string(&global_hooks).unwrap(),
        "{\"existing\":{}}\n"
    );

    let plugin_dir = gemini.join("config/plugins").join(PLUGIN_NAME);
    let manifest: Value =
        serde_json::from_slice(&fs::read(plugin_dir.join("plugin.json")).unwrap()).unwrap();
    assert_eq!(manifest["name"], PLUGIN_NAME);
    let hooks_doc: Value =
        serde_json::from_slice(&fs::read(plugin_dir.join("hooks.json")).unwrap()).unwrap();
    let key_a = hook_group_key(&cfg_a);
    let key_b = hook_group_key(&cfg_b);
    assert!(hooks_doc[&key_a]["Stop"].is_array());
    assert!(hooks_doc[&key_b]["SessionStart"].is_array());

    // Uninstalling app-a retains app-b
    register(&cfg_a, &gemini, false).unwrap();
    let after_a: Value =
        serde_json::from_slice(&fs::read(plugin_dir.join("hooks.json")).unwrap()).unwrap();
    assert!(after_a.get(&key_a).is_none());
    assert!(after_a.get(&key_b).is_some());

    // Uninstalling app-b removes the empty plugin directory completely
    register(&cfg_b, &gemini, false).unwrap();
    assert!(!plugin_dir.exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn register_refuses_foreign_plugin_ownership() {
    let dir = temp_dir("foreign");
    let gemini = dir.join("gemini");
    let plugin_dir = gemini.join("config/plugins").join(PLUGIN_NAME);
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{"name":"someone-else"}"#,
    )
    .unwrap();

    let cfg = dir.join("app/config.json");
    configure(
        &cfg,
        "https://example.invalid/api/logs/v1/events".into(),
        &gemini,
        "k".into(),
    )
    .unwrap();
    assert!(register(&cfg, &gemini, true).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hook_command_handles_spaces_and_windows_cmd_quoting() {
    let bin = PathBuf::from(if cfg!(windows) {
        r"C:\Program Files\1flowbase\antigravity-logs-collector.exe"
    } else {
        "/opt/with 'quote'/antigravity-logs-collector"
    });
    let cfg = PathBuf::from(if cfg!(windows) {
        r"C:\Users\Test User\AppData\Local\1flowbase\config.json"
    } else {
        "/tmp/my config/config.json"
    });
    let key = hook_group_key(&cfg);
    let cmd = hook_command(&bin, &cfg, &key).unwrap();
    if cfg!(windows) {
        assert!(!cmd.contains('"'));
        assert!(!cmd.contains(' '));
        assert!(cmd.ends_with(".cmd"));
        let wrapper = render_windows_cmd_wrapper(&bin, &cfg).unwrap();
        assert!(wrapper.contains("hook --config"));
        assert!(wrapper.contains(r#""C:\Program Files\1flowbase\antigravity-logs-collector.exe""#));
    } else {
        assert!(cmd.contains("hook --config"));
    }
}
