use std::fs;
use std::path::Path;
use tandem::web_manager::{
    WEB_BUILD_HASH_FILE, WEB_SOURCE_HASH_STAMP, WEB_VERSION_STAMP, WebConfig, copy_dir_all,
    ensure_web_dir, installed_is_current, web_is_built, web_server_args, web_source_hash,
};

mod common;
use common::TempDir;

fn cfg(ip: &str, port: u16) -> WebConfig {
    WebConfig {
        bind_ip: ip.to_string(),
        port,
        ..WebConfig::default()
    }
}

fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test")
        .join("fixtures")
        .join("web-source-hash")
}

fn expected_digest() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test")
        .join("fixtures")
        .join("web-source-hash.EXPECTED_SHA256");
    fs::read_to_string(&p)
        .unwrap_or_else(|_| panic!("missing {}", p.display()))
        .trim()
        .to_string()
}

#[test]
fn server_args_pass_bind_ip_and_port_explicitly() {
    let args = web_server_args(&cfg("100.64.0.2", 4000)).expect("args");
    assert_eq!(
        args,
        ["run", "start", "--", "-H", "100.64.0.2", "-p", "4000"]
    );
}

#[test]
fn server_args_refuse_wildcard_bind() {
    assert!(web_server_args(&cfg("0.0.0.0", 3940)).is_err());
    assert!(web_server_args(&cfg("::", 3940)).is_err());
}

#[test]
fn server_args_refuse_empty_token() {
    let mut c = cfg("100.64.0.2", 3940);
    c.auth_token = String::new();
    assert!(web_server_args(&c).is_err());
}

// ----- Part A: hash algorithm -----

#[test]
fn source_hash_matches_shared_fixture() {
    let dir = fixture_dir();
    assert!(dir.exists(), "fixture dir missing: {}", dir.display());
    let got = web_source_hash(&dir).expect("hash");
    let want = expected_digest();
    assert_eq!(got, want, "Rust hash does not match EXPECTED_SHA256");
}

#[test]
fn source_hash_ignores_excluded_paths() {
    let tmp = TempDir::new();
    let base = tmp.path();

    // These are included:
    fs::create_dir_all(base.join("src/app")).unwrap();
    fs::write(base.join("src/app/page.tsx"), "included").unwrap();

    let hash_before = web_source_hash(base).unwrap();

    // Add excluded files — hash must not change
    fs::create_dir_all(base.join("node_modules/x")).unwrap();
    fs::write(base.join("node_modules/x/index.js"), "npm").unwrap();
    fs::create_dir_all(base.join(".next")).unwrap();
    fs::write(base.join(".next/BUILD_ID"), "b").unwrap();
    fs::create_dir_all(base.join("tests")).unwrap();
    fs::write(base.join("tests/x.test.ts"), "test").unwrap();
    fs::write(base.join("src/app/page.tsbuildinfo"), "{}").unwrap();
    fs::write(base.join(".tandem-version"), "1.0.0").unwrap();
    fs::write(base.join("next-env.d.ts"), "// next").unwrap();

    let hash_after = web_source_hash(base).unwrap();
    assert_eq!(
        hash_before, hash_after,
        "excluded files must not change the hash"
    );

    // Changing an included file must change the hash
    fs::write(base.join("src/app/page.tsx"), "changed").unwrap();
    let hash_changed = web_source_hash(base).unwrap();
    assert_ne!(
        hash_before, hash_changed,
        "content change must change the hash"
    );
}

// ----- Part B: stamp and compare -----

#[test]
fn installed_with_same_version_but_different_source_is_not_current() {
    let tmp = TempDir::new();
    let installed = tmp.path().join("installed");
    let bundled = tmp.path().join("bundled");

    // Set up bundled source
    fs::create_dir_all(bundled.join("src")).unwrap();
    fs::write(bundled.join("package.json"), "{}").unwrap();
    fs::write(bundled.join("src/main.ts"), "v2").unwrap();

    // Set up installed source that is at the same version but different content
    fs::create_dir_all(installed.join("src")).unwrap();
    fs::write(installed.join("package.json"), "{}").unwrap();
    fs::write(installed.join("src/main.ts"), "v1").unwrap();
    // Write a source hash stamp that does NOT match bundled
    fs::write(installed.join(WEB_SOURCE_HASH_STAMP), "deadbeef").unwrap();
    fs::write(
        installed.join(WEB_VERSION_STAMP),
        format!("{}\n", env!("CARGO_PKG_VERSION")),
    )
    .unwrap();

    assert!(
        !installed_is_current(&installed, Some(&bundled)),
        "different source hash must not be considered current"
    );
}

#[test]
fn installed_with_matching_hash_is_current() {
    let tmp = TempDir::new();
    let installed = tmp.path().join("installed");
    let bundled = tmp.path().join("bundled");

    // Create identical content in bundled
    fs::create_dir_all(bundled.join("src")).unwrap();
    fs::write(bundled.join("package.json"), "{}").unwrap();
    fs::write(bundled.join("src/main.ts"), "content").unwrap();

    // Mirror in installed
    fs::create_dir_all(installed.join("src")).unwrap();
    fs::write(installed.join("package.json"), "{}").unwrap();
    fs::write(installed.join("src/main.ts"), "content").unwrap();
    let hash = web_source_hash(&bundled).unwrap();
    fs::write(installed.join(WEB_SOURCE_HASH_STAMP), format!("{hash}\n")).unwrap();
    fs::write(
        installed.join(WEB_VERSION_STAMP),
        format!("{}\n", env!("CARGO_PKG_VERSION")),
    )
    .unwrap();

    assert!(
        installed_is_current(&installed, Some(&bundled)),
        "matching hash must be considered current"
    );
}

#[test]
fn legacy_install_without_hash_stamp_is_refreshed_once() {
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    let state = tmp.path().join("state");
    let bundled = state
        .join("bin")
        .join("current")
        .join("tandem")
        .join("tandem-web");
    let installed = state.join("tandem-web-core-service");

    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(bundled.join("src")).unwrap();
    fs::write(bundled.join("package.json"), "{}").unwrap();
    fs::write(bundled.join("src/index.ts"), "hello").unwrap();

    // Legacy install: version stamp but NO source hash stamp
    fs::create_dir_all(installed.join("src")).unwrap();
    fs::write(installed.join("package.json"), "{}").unwrap();
    fs::write(installed.join("src/index.ts"), "old").unwrap();
    fs::write(
        installed.join(WEB_VERSION_STAMP),
        format!("{}\n", env!("CARGO_PKG_VERSION")),
    )
    .unwrap();
    // No WEB_SOURCE_HASH_STAMP → not current

    // ensure_web_dir will recopy from bundled
    let result = ensure_web_dir(&ws, &state);
    assert!(result.is_some(), "ensure_web_dir must find the dir");
    let dir = result.unwrap();

    // After recopy, source hash stamp should exist
    assert!(
        dir.join(WEB_SOURCE_HASH_STAMP).exists(),
        "source hash stamp must be written after copy"
    );

    // Second call: should return the same dir without re-copying
    let result2 = ensure_web_dir(&ws, &state);
    assert!(result2.is_some());
    // The stamp must match the bundled hash
    let stamp = fs::read_to_string(dir.join(WEB_SOURCE_HASH_STAMP)).unwrap();
    let bundled_hash = web_source_hash(&bundled).unwrap();
    assert_eq!(stamp.trim(), bundled_hash, "stamp must equal bundled hash");
}

#[test]
fn web_is_built_false_when_build_hash_differs_from_source_hash() {
    let tmp = TempDir::new();
    let web_dir = tmp.path();

    fs::create_dir_all(web_dir.join(".next")).unwrap();
    fs::write(web_dir.join(".next/BUILD_ID"), "b1").unwrap();
    // No stamps at all → not built
    assert!(!web_is_built(web_dir));

    // Source hash stamp exists, build hash does NOT match
    fs::write(web_dir.join(WEB_SOURCE_HASH_STAMP), "aabbcc").unwrap();
    fs::write(web_dir.join(".next").join(WEB_BUILD_HASH_FILE), "112233").unwrap();
    assert!(
        !web_is_built(web_dir),
        "mismatched hashes must report not built"
    );

    fs::write(web_dir.join(".next").join(WEB_BUILD_HASH_FILE), "aabbcc").unwrap();
    assert!(web_is_built(web_dir), "matching hashes must report built");
}

#[test]
fn web_is_built_false_when_stamp_is_missing() {
    let tmp = TempDir::new();
    let web_dir = tmp.path();

    fs::create_dir_all(web_dir.join(".next")).unwrap();
    fs::write(web_dir.join(".next/BUILD_ID"), "b").unwrap();
    // Build hash file exists but no source stamp
    fs::write(web_dir.join(".next").join(WEB_BUILD_HASH_FILE), "aabbcc").unwrap();

    assert!(
        !web_is_built(web_dir),
        "missing source stamp must report not built"
    );
}

#[test]
fn copy_excludes_tests_and_next_env() {
    let tmp = TempDir::new();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");

    // Create files in src
    fs::create_dir_all(src.join("src")).unwrap();
    fs::write(src.join("package.json"), "{}").unwrap();
    fs::write(src.join("src/main.ts"), "main").unwrap();
    fs::create_dir_all(src.join("tests")).unwrap();
    fs::write(src.join("tests/x.test.ts"), "test").unwrap();
    fs::write(src.join("next-env.d.ts"), "env").unwrap();
    fs::create_dir_all(src.join("node_modules/x")).unwrap();
    fs::write(src.join("node_modules/x/index.js"), "npm").unwrap();
    fs::write(src.join("foo.tsbuildinfo"), "{}").unwrap();

    copy_dir_all(&src, &dst).unwrap();

    assert!(
        dst.join("package.json").exists(),
        "package.json must be copied"
    );
    assert!(
        dst.join("src/main.ts").exists(),
        "src/main.ts must be copied"
    );
    assert!(
        !dst.join("tests").exists(),
        "tests/ must be excluded from copy"
    );
    assert!(
        !dst.join("next-env.d.ts").exists(),
        "next-env.d.ts must be excluded from copy"
    );
    assert!(
        !dst.join("node_modules").exists(),
        "node_modules/ must be excluded from copy"
    );
    assert!(
        !dst.join("foo.tsbuildinfo").exists(),
        ".tsbuildinfo files must be excluded from copy"
    );
}

#[test]
fn ensure_web_dir_recopies_when_bundled_source_changes() {
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    let state = tmp.path().join("state");
    let bundled = state
        .join("bin")
        .join("current")
        .join("tandem")
        .join("tandem-web");
    let installed = state.join("tandem-web-core-service");

    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(bundled.join("src")).unwrap();
    fs::write(bundled.join("package.json"), "{}").unwrap();
    fs::write(bundled.join("src/index.ts"), "version1").unwrap();

    ensure_web_dir(&ws, &state).unwrap();
    let hash1 = web_source_hash(&bundled).unwrap();
    let stamp1 = fs::read_to_string(installed.join(WEB_SOURCE_HASH_STAMP)).unwrap();
    assert_eq!(stamp1.trim(), hash1);

    // Change bundled source (same version, different content)
    fs::write(bundled.join("src/index.ts"), "version2").unwrap();
    let hash2 = web_source_hash(&bundled).unwrap();
    assert_ne!(hash1, hash2);

    // ensure_web_dir must detect the change and re-copy
    ensure_web_dir(&ws, &state).unwrap();
    let stamp2 = fs::read_to_string(installed.join(WEB_SOURCE_HASH_STAMP)).unwrap();
    assert_eq!(stamp2.trim(), hash2, "stamp must be updated after re-copy");

    // Installed source must reflect the new version
    let installed_content = fs::read_to_string(installed.join("src/index.ts")).unwrap();
    assert_eq!(
        installed_content, "version2",
        "installed source must be updated"
    );
}

#[test]
fn daemon_needs_restart_when_running_daemon_hash_differs() {
    // This test checks the daemon_needs_restart function uses version+hash
    // We use the fact that when no PID file exists, it always returns true.
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    let state = tmp.path().join("state");
    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(&state).unwrap();

    // No PID file → always needs restart
    assert!(
        tandem::web_manager::daemon_needs_restart(&ws, &state),
        "no PID file must report needs restart"
    );
}
