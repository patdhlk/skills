/// Asserts that the pds-cli crate version equals the version field in
/// .claude-plugin/plugin.json at the repo root.
///
/// Both must be bumped together in every release commit.  This test runs as
/// part of `cargo test --all`, so the existing CI step enforces the invariant
/// automatically whenever cli/** or .claude-plugin/plugin.json changes.
///
/// # Published-crate behaviour
///
/// When the crate is installed from crates.io there is no `.git` directory, so
/// the walk-up search terminates without finding a repo root.  The test SKIPS
/// (prints an explanatory message and returns) rather than panicking — the
/// invariant only needs to hold inside the source repository.
///
/// # In-repo invariants
///
/// * `.git` found AND `.claude-plugin/plugin.json` present → versions must match.
/// * `.git` found BUT `.claude-plugin/plugin.json` missing → **FAIL** (the file
///   was deleted or the test is running from an unexpected subtree).
#[test]
fn plugin_json_version_matches_crate_version() {
    // Walk up from CARGO_MANIFEST_DIR looking for a directory that contains
    // `.git`.  Bounded to ≤ 10 levels to avoid runaway traversal.
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let repo_root = {
        let mut candidate = manifest_dir.as_path();
        let mut found = None;
        for _ in 0..=10 {
            if candidate.join(".git").exists() {
                found = Some(candidate.to_path_buf());
                break;
            }
            match candidate.parent() {
                Some(p) => candidate = p,
                None => break,
            }
        }
        found
    };

    let repo_root = match repo_root {
        Some(r) => r,
        None => {
            // No .git found — we are running from a published crate on crates.io
            // or some other context without a repository.  Skip gracefully.
            println!(
                "version_lockstep: no .git directory found within 10 levels of {}; \
                 assuming published-crate context — skipping invariant check.",
                manifest_dir.display()
            );
            return;
        }
    };

    // .git IS present — the invariant must hold.  Fail loudly if plugin.json
    // is missing (indicates a structural problem in the repo).
    let plugin_json = repo_root.join(".claude-plugin/plugin.json");
    assert!(
        plugin_json.exists(),
        "Found repo root at {} (.git present) but .claude-plugin/plugin.json is missing. \
         Both the crate version and plugin.json version must be kept in sync; \
         restoring or creating the file is required.",
        repo_root.display()
    );

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
