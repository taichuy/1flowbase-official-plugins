use super::*;
#[test]
fn subagent_hook_never_selects_the_parent_transcript() {
    let mut input = json!({"hook_event_name":"SubagentStop","transcript_path":"/fixture/parent.jsonl","agent_transcript_path":"/fixture/child.jsonl"});
    assert_eq!(
        selected_transcript(&input).unwrap(),
        Some(PathBuf::from("/fixture/child.jsonl"))
    );
    input
        .as_object_mut()
        .unwrap()
        .remove("agent_transcript_path");
    assert_eq!(selected_transcript(&input).unwrap(), None);
    input["hook_event_name"] = json!("Stop");
    assert_eq!(
        selected_transcript(&input).unwrap(),
        Some(PathBuf::from("/fixture/parent.jsonl"))
    );
    input["hook_event_name"] = json!("unsupported");
    assert!(selected_transcript(&input).is_err());
}
#[test]
fn installation_is_idempotent_and_preserves_unrelated_handlers_and_settings() {
    let original = json!({"model":"existing","permissions":{"allow":["Read"]},"disableAllHooks":false,"hooks":{"Stop":[{"matcher":"x","hooks":[{"type":"command","command":"existing","statusMessage":"other"}]}],"Notification":[{"hooks":[{"type":"command","command":"notify"}]}]}});
    let mut doc = original.clone();
    merge_settings(&mut doc, "owned", "collector hook", true).unwrap();
    let once = doc.clone();
    merge_settings(&mut doc, "owned", "collector hook", true).unwrap();
    assert_eq!(once, doc);
    assert_eq!(doc["model"], original["model"]);
    assert_eq!(doc["permissions"], original["permissions"]);
    assert_eq!(
        doc["hooks"]["Notification"],
        original["hooks"]["Notification"]
    );
    assert_eq!(doc["hooks"]["Stop"][0], original["hooks"]["Stop"][0]);
    merge_settings(&mut doc, "owned", "collector hook", false).unwrap();
    assert_eq!(doc["hooks"]["Stop"], original["hooks"]["Stop"]);
    assert!(EVENTS
        .iter()
        .filter(|e| **e != "Stop")
        .all(|e| doc["hooks"][*e].as_array().unwrap().is_empty()));
}
#[test]
fn reinstall_changes_only_this_config_identity() {
    let mut doc = json!({});
    merge_settings(&mut doc, "installation-a", "old", true).unwrap();
    merge_settings(&mut doc, "installation-b", "other", true).unwrap();
    merge_settings(&mut doc, "installation-a", "new", true).unwrap();
    assert_eq!(doc["hooks"]["Stop"].as_array().unwrap().len(), 2);
    merge_settings(&mut doc, "installation-a", "new", false).unwrap();
    assert_eq!(doc["hooks"]["Stop"][0]["hooks"][0]["command"], "other");
}
#[test]
fn malformed_settings_fail_before_register_can_write() {
    assert!(merge_settings(&mut json!({"hooks":{"Stop":{}}}), "owned", "cmd", true).is_err());
    assert!(merge_settings(
        &mut json!({"hooks":{"Stop":[{"hooks":"bad"}]}}),
        "owned",
        "cmd",
        true
    )
    .is_err());
    assert!(merge_settings(&mut json!([]), "owned", "cmd", true).is_err());
}
#[test]
fn command_keeps_apostrophes_and_spaces_in_a_single_argument() {
    if !cfg!(windows) {
        assert_eq!(
            command(Path::new("/tmp/a b/bin"), Path::new("/tmp/c'd/config")),
            "'/tmp/a b/bin' hook --config '/tmp/c'\\''d/config'"
        );
    } else {
        assert!(command(Path::new("C:/a b/bin.exe"), Path::new("C:/config"))
            .contains("-EncodedCommand"));
    }
}
