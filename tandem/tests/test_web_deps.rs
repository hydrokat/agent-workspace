mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use common::TempDir;
use tandem::web_deps::{
    BunState, DepsEnv, Facts, HostProbe, Plan, Platform, RunResult, check_web_deps,
    detect_host_problem, find_executable, plan_from_facts, platform_from, platform_label,
};

fn plat(os: &str, arch: &str, libc: Option<&str>) -> Platform {
    Platform {
        os: os.into(),
        arch: arch.into(),
        libc: libc.map(String::from),
    }
}

fn facts(platform: Platform) -> Facts {
    Facts {
        platform,
        bun: BunState::Missing,
        ttyd: false,
        need_terminal: false,
        host_problem: None,
        tools: vec!["curl".into(), "unzip".into()],
    }
}

fn tools(f: &mut Facts, list: &[&str]) {
    f.tools = list.iter().map(|s| s.to_string()).collect();
}

#[test]
fn parity_fixture_matches_the_node_planner_for_every_case() {
    let raw = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity/web-deps.json"),
    )
    .unwrap();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    assert!(cases.len() >= 16, "fixture should keep its coverage");
    for case in cases {
        let name = case["name"].as_str().unwrap().to_string();
        let facts: Facts = serde_json::from_value(case["facts"].clone())
            .unwrap_or_else(|e| panic!("{name}: facts do not parse: {e}"));
        let plan = plan_from_facts(&facts);
        assert_eq!(
            serde_json::to_value(&plan).unwrap(),
            case["expected"],
            "plan differs from the shared fixture for {name}"
        );
    }
}

#[test]
fn nothing_to_do_when_bun_runs() {
    let mut f = facts(plat("macos", "arm64", None));
    f.bun = BunState::Ok;
    let plan = plan_from_facts(&f);
    assert!(!plan.bun_missing && plan.auto_install.is_empty() && plan.manual.is_empty());
    assert!(plan.script.is_none() && !plan.can_auto_install);
}

#[test]
fn macos_script_uses_the_official_bun_com_installer() {
    let plan = plan_from_facts(&facts(plat("macos", "arm64", None)));
    let script = plan.script.expect("script");
    assert_eq!(script.name, "install-web-deps.sh");
    assert!(script.content.starts_with("#!/usr/bin/env bash\n"));
    assert!(script.content.contains("for macOS arm64"));
    assert!(
        script
            .content
            .contains("curl -fsSL https://bun.com/install | bash")
    );
    assert!(script.content.contains("set -euo pipefail"));
    assert!(!script.content.contains("bun.sh/install\n"));
    assert!(!script.content.contains("ttyd"));
}

#[test]
fn scripts_never_use_elevated_privileges() {
    let mut ttyd = facts(plat("macos", "x64", None));
    ttyd.bun = BunState::Ok;
    ttyd.need_terminal = true;
    tools(&mut ttyd, &["brew"]);
    for f in [
        facts(plat("macos", "arm64", None)),
        facts(plat("linux", "x64", Some("glibc"))),
        facts(plat("windows", "x64", None)),
        ttyd,
    ] {
        if let Some(script) = plan_from_facts(&f).script {
            let lower = script.content.to_lowercase();
            assert!(
                !lower.contains("sudo") && !lower.contains("runas"),
                "{lower}"
            );
        }
    }
}

#[test]
fn linux_without_unzip_cannot_auto_install_and_gets_the_distro_command() {
    let mut f = facts(plat("linux", "x64", Some("glibc")));
    tools(&mut f, &["curl", "apt"]);
    let plan = plan_from_facts(&f);
    assert!(!plan.can_auto_install && plan.script.is_none());
    assert_eq!(plan.manual[0].dep, "unzip");
    assert_eq!(plan.manual[0].commands, ["sudo apt install unzip"]);
    assert_eq!(plan.manual[1].dep, "bun");

    tools(&mut f, &["curl", "dnf"]);
    assert_eq!(
        plan_from_facts(&f).manual[0].commands,
        ["sudo dnf install unzip"]
    );
    tools(&mut f, &["curl"]);
    assert_eq!(
        plan_from_facts(&f).manual[0].commands,
        ["sudo apt install unzip"]
    );
}

#[test]
fn unsupported_platform_and_host_problems_are_reported_not_installed() {
    let plan = plan_from_facts(&facts(plat("freebsd", "riscv64", None)));
    assert!(!plan.supported && plan.script.is_none());
    assert!(plan.unsupported_reason.unwrap().contains("freebsd riscv64"));
    assert!(plan.manual[0].commands[0].contains("https://bun.com/docs/installation"));

    let mut old = facts(plat("macos", "x64", None));
    old.host_problem = Some("macOS 12.7 is older than 13.0".into());
    let plan = plan_from_facts(&old);
    assert!(!plan.supported);
    assert_eq!(
        plan.unsupported_reason.as_deref(),
        Some("macOS 12.7 is older than 13.0")
    );
}

#[test]
fn a_bun_that_does_not_run_is_treated_as_missing() {
    let mut f = facts(plat("linux", "arm64", Some("glibc")));
    f.bun = BunState::Broken;
    let plan = plan_from_facts(&f);
    assert!(plan.bun_missing && plan.bun_broken);
    assert_eq!(plan.auto_install, ["bun"]);
}

#[test]
fn ttyd_is_only_considered_when_the_terminal_feature_is_on() {
    let mut f = facts(plat("macos", "arm64", None));
    f.bun = BunState::Ok;
    assert!(!plan_from_facts(&f).ttyd_missing);

    f.need_terminal = true;
    tools(&mut f, &["brew"]);
    let plan = plan_from_facts(&f);
    assert_eq!(plan.auto_install, ["ttyd"]);
    assert!(!plan.script.unwrap().content.contains("Installing bun"));

    let mut linux = facts(plat("linux", "x64", Some("glibc")));
    linux.bun = BunState::Ok;
    linux.need_terminal = true;
    tools(&mut linux, &["apt"]);
    let plan = plan_from_facts(&linux);
    assert!(plan.auto_install.is_empty());
    assert_eq!(plan.manual[0].commands, ["sudo apt install ttyd"]);
}

#[test]
fn platform_labels_and_mapping() {
    assert_eq!(platform_label(&plat("macos", "arm64", None)), "macOS arm64");
    assert_eq!(
        platform_label(&plat("linux", "x64", Some("musl"))),
        "Linux x64 (musl)"
    );
    assert_eq!(
        platform_from("macos", "aarch64", |_| false),
        plat("macos", "arm64", None)
    );
    assert_eq!(
        platform_from("macos", "x86_64", |_| false),
        plat("macos", "x64", None)
    );
    assert_eq!(
        platform_from("linux", "x86_64", |_| false),
        plat("linux", "x64", Some("glibc"))
    );
    assert_eq!(
        platform_from("linux", "aarch64", |p| p == "/lib/ld-musl-aarch64.so.1"),
        plat("linux", "arm64", Some("musl"))
    );
    assert_eq!(
        platform_from("freebsd", "riscv64", |_| false),
        plat("freebsd", "riscv64", None)
    );
}

struct FakeProbe {
    run_ok: bool,
    stdout: String,
    cpuinfo: Option<String>,
    ran: Mutex<Vec<String>>,
}

impl FakeProbe {
    fn new(run_ok: bool, stdout: &str) -> Self {
        Self {
            run_ok,
            stdout: stdout.into(),
            cpuinfo: Some("flags : sse4_2 avx2".into()),
            ran: Mutex::new(Vec::new()),
        }
    }
}

impl HostProbe for FakeProbe {
    fn run(&self, cmd: &Path, args: &[&str]) -> RunResult {
        self.ran
            .lock()
            .unwrap()
            .push(format!("{} {}", cmd.display(), args.join(" ")));
        RunResult {
            ok: self.run_ok,
            stdout: self.stdout.clone(),
        }
    }
    fn read_file(&self, _: &str) -> Option<String> {
        self.cpuinfo.clone()
    }
}

#[test]
fn host_problem_detection() {
    let old = FakeProbe::new(true, "12.7.4\n");
    assert!(
        detect_host_problem(&plat("macos", "arm64", None), &old)
            .unwrap()
            .contains("macOS 12.7.4")
    );
    let new = FakeProbe::new(true, "14.5\n");
    assert_eq!(
        detect_host_problem(&plat("macos", "arm64", None), &new),
        None
    );
    let unknown = FakeProbe::new(false, "");
    assert_eq!(
        detect_host_problem(&plat("macos", "arm64", None), &unknown),
        None
    );

    let mut no_sse = FakeProbe::new(true, "");
    no_sse.cpuinfo = Some("flags : fpu sse2".into());
    assert!(
        detect_host_problem(&plat("linux", "x64", Some("glibc")), &no_sse)
            .unwrap()
            .contains("SSE4.2")
    );
    let ok = FakeProbe::new(true, "");
    assert_eq!(
        detect_host_problem(&plat("linux", "x64", Some("glibc")), &ok),
        None
    );
    let mut unreadable = FakeProbe::new(true, "");
    unreadable.cpuinfo = None;
    assert_eq!(
        detect_host_problem(&plat("linux", "x64", Some("glibc")), &unreadable),
        None
    );
    assert_eq!(
        detect_host_problem(&plat("linux", "arm64", Some("glibc")), &no_sse),
        None
    );
}

struct Sandbox {
    _tmp: TempDir,
    bin: PathBuf,
    home: PathBuf,
}

fn sandbox() -> Sandbox {
    let tmp = TempDir::new();
    let bin = tmp.path().join("bin");
    let home = tmp.path().join("home");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(home.join(".bun/bin")).unwrap();
    Sandbox {
        _tmp: tmp,
        bin,
        home,
    }
}

fn exe(path: &Path) {
    fs::write(path, "#!/bin/sh\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn env_for(s: &Sandbox) -> DepsEnv {
    DepsEnv {
        path: s.bin.to_string_lossy().to_string(),
        home: s.home.clone(),
        bun_install: None,
    }
}

#[test]
fn finds_executables_on_path_and_in_extra_dirs() {
    let s = sandbox();
    exe(&s.bin.join("curl"));
    let path = s.bin.to_string_lossy().to_string();
    assert_eq!(
        find_executable("curl", &path, &[]),
        Some(s.bin.join("curl"))
    );
    assert_eq!(find_executable("nope", &path, &[]), None);

    exe(&s.home.join(".bun/bin/bun"));
    let extra = [s.home.join(".bun/bin")];
    assert_eq!(
        find_executable("bun", &path, &extra),
        Some(s.home.join(".bun/bin/bun"))
    );
    assert_eq!(find_executable("bun", &path, &[]), None);
}

#[test]
fn a_non_executable_file_is_not_found() {
    let s = sandbox();
    fs::write(s.bin.join("curl"), "x").unwrap();
    let path = s.bin.to_string_lossy().to_string();
    assert_eq!(find_executable("curl", &path, &[]), None);
}

#[test]
fn bun_that_runs_is_ok_with_its_version() {
    let s = sandbox();
    exe(&s.bin.join("bun"));
    let probe = FakeProbe::new(true, "1.3.3\n");
    let r = check_web_deps(&env_for(&s), plat("macos", "arm64", None), false, &probe);
    assert_eq!(r.facts.bun, BunState::Ok);
    assert_eq!(r.bun_version.as_deref(), Some("1.3.3"));
    assert_eq!(r.bun_path, Some(s.bin.join("bun")));
    assert!(
        probe
            .ran
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.ends_with("bun --version"))
    );
}

#[test]
fn bun_that_does_not_run_is_broken_and_absent_bun_is_missing() {
    let s = sandbox();
    exe(&s.bin.join("bun"));
    let broken = check_web_deps(
        &env_for(&s),
        plat("macos", "arm64", None),
        false,
        &FakeProbe::new(false, ""),
    );
    assert_eq!(broken.facts.bun, BunState::Broken);
    assert_eq!(broken.bun_path, None);

    let empty = sandbox();
    let missing = check_web_deps(
        &env_for(&empty),
        plat("macos", "arm64", None),
        false,
        &FakeProbe::new(true, ""),
    );
    assert_eq!(missing.facts.bun, BunState::Missing);
}

#[test]
fn bun_in_the_default_install_dir_is_found_without_a_path_entry() {
    let s = sandbox();
    exe(&s.home.join(".bun/bin/bun"));
    let r = check_web_deps(
        &env_for(&s),
        plat("linux", "x64", Some("glibc")),
        false,
        &FakeProbe::new(true, "1.3.3\n"),
    );
    assert_eq!(r.facts.bun, BunState::Ok);
    assert_eq!(r.bun_path, Some(s.home.join(".bun/bin/bun")));
}

#[test]
fn bun_install_env_var_is_honoured() {
    let s = sandbox();
    let custom = s.home.join("custom");
    fs::create_dir_all(custom.join("bin")).unwrap();
    exe(&custom.join("bin/bun"));
    let mut env = env_for(&s);
    env.bun_install = Some(custom.clone());
    let r = check_web_deps(
        &env,
        plat("linux", "x64", Some("glibc")),
        false,
        &FakeProbe::new(true, "1.3.3\n"),
    );
    assert_eq!(r.bun_path, Some(custom.join("bin/bun")));
}

#[test]
fn ttyd_is_checked_only_when_requested_and_tools_are_recorded() {
    let s = sandbox();
    for t in ["curl", "unzip", "brew", "apt"] {
        exe(&s.bin.join(t));
    }
    let probe = FakeProbe::new(true, "");
    let off = check_web_deps(
        &env_for(&s),
        plat("linux", "x64", Some("glibc")),
        false,
        &probe,
    );
    assert!(!off.facts.need_terminal);
    let mut got = off.facts.tools.clone();
    got.sort();
    assert_eq!(got, ["apt", "brew", "curl", "unzip"]);

    exe(&s.bin.join("ttyd"));
    let on = check_web_deps(
        &env_for(&s),
        plat("linux", "x64", Some("glibc")),
        true,
        &probe,
    );
    assert!(on.facts.ttyd && on.facts.need_terminal);
}

#[test]
fn plan_is_serialisable_for_the_parity_fixture() {
    let plan: Plan = plan_from_facts(&facts(plat("macos", "arm64", None)));
    let v = serde_json::to_value(&plan).unwrap();
    for key in [
        "supported",
        "unsupportedReason",
        "bunMissing",
        "bunBroken",
        "ttydMissing",
        "canAutoInstall",
        "autoInstall",
        "manual",
        "script",
    ] {
        assert!(v.get(key).is_some(), "missing key {key}");
    }
}

#[test]
fn locate_bun_prefers_path_then_the_default_install_dir() {
    use tandem::web_deps::locate_bun;
    let s = sandbox();
    assert_eq!(locate_bun(&env_for(&s)), None);

    exe(&s.home.join(".bun/bin/bun"));
    assert_eq!(locate_bun(&env_for(&s)), Some(s.home.join(".bun/bin/bun")));

    exe(&s.bin.join("bun"));
    assert_eq!(locate_bun(&env_for(&s)), Some(s.bin.join("bun")));
}

#[test]
fn every_test_that_drives_the_web_daemon_also_fakes_the_dependency_check() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let body = fs::read_to_string(&path).unwrap();
        if body.contains(".with_web_daemon(") && !body.contains(".with_deps_runtime(") {
            offenders.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "these tests enable web through the real host check and would fail on a machine without bun: {offenders:?}"
    );
}
