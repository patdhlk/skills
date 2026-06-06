/// Asserts that the pds-cli crate version equals the version field in
/// .claude-plugin/plugin.json at the repo root.
///
/// Both must be bumped together in every release commit.  This test runs as
/// part of `cargo test --all`, so the existing CI step enforces the invariant
/// automatically whenever cli/** or .claude-plugin/plugin.json changes.
#[test]
fn plugin_json_version_matches_crate_version() {
    // CARGO_MANIFEST_DIR is set by the Rust test harness to the manifest dir
    // of the crate under test (cli/pds-cli/).  Walk two levels up to reach
    // the repo root (skills/), then descend into .claude-plugin/plugin.json.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin_json = manifest_dir
        .join("../../.claude-plugin/plugin.json")
        .canonicalize()
        .expect(".claude-plugin/plugin.json must exist relative to cli/pds-cli/");

    let raw = std::fs::read_to_string(&plugin_json)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", plugin_json.display()));

    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("plugin.json must be valid JSON");

    let plugin_version = value["version"]
        .as_str()
        .expect("plugin.json must have a string \"version\" field");

    let crate_version = env!("CARGO_PKG_VERSION");

    assert_eq!(
        crate_version, plugin_version,
        "Version mismatch: pds-cli crate version is \"{crate_version}\" \
         but .claude-plugin/plugin.json version is \"{plugin_version}\". \
         Both must be updated together in a single release commit \
         (workspace Cargo.toml [workspace.package] version + plugin.json .version)."
    );
}
