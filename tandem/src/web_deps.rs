use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

pub const BUN_DOCS_URL: &str = "https://bun.com/docs/installation";
const BUN_INSTALL_CMD: &str = "curl -fsSL https://bun.com/install | bash";
const BUN_INSTALL_PS: &str = "powershell -c \"irm bun.sh/install.ps1|iex\"";
const PACKAGE_MANAGERS: [&str; 6] = ["apt", "dnf", "yum", "pacman", "apk", "zypper"];
const KNOWN_TOOLS: [&str; 10] = [
    "curl", "unzip", "brew", "scoop", "apt", "dnf", "yum", "pacman", "apk", "zypper",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub libc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BunState {
    Ok,
    Missing,
    Broken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub platform: Platform,
    pub bun: BunState,
    pub ttyd: bool,
    pub need_terminal: bool,
    #[serde(default)]
    pub host_problem: Option<String>,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualStep {
    pub dep: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Script {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub supported: bool,
    pub unsupported_reason: Option<String>,
    pub bun_missing: bool,
    pub bun_broken: bool,
    pub ttyd_missing: bool,
    pub can_auto_install: bool,
    pub auto_install: Vec<String>,
    pub manual: Vec<ManualStep>,
    pub script: Option<Script>,
}

pub fn platform_label(platform: &Platform) -> String {
    let os = match platform.os.as_str() {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };
    match &platform.libc {
        Some(libc) => format!("{os} {} ({libc})", platform.arch),
        None => format!("{os} {}", platform.arch),
    }
}

pub fn platform_from(os: &str, arch: &str, musl_loader_exists: impl Fn(&str) -> bool) -> Platform {
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let libc = if os == "linux" {
        let loader = if arch == "arm64" { "aarch64" } else { "x86_64" };
        if musl_loader_exists(&format!("/lib/ld-musl-{loader}.so.1")) {
            Some("musl".to_string())
        } else {
            Some("glibc".to_string())
        }
    } else {
        None
    };
    Platform {
        os: os.to_string(),
        arch: arch.to_string(),
        libc,
    }
}

pub fn detect_platform() -> Platform {
    platform_from(std::env::consts::OS, std::env::consts::ARCH, |p| {
        Path::new(p).exists()
    })
}

pub struct RunResult {
    pub ok: bool,
    pub stdout: String,
}

pub trait HostProbe {
    fn run(&self, cmd: &Path, args: &[&str]) -> RunResult;
    fn read_file(&self, path: &str) -> Option<String>;
}

pub struct SystemProbe;

impl HostProbe for SystemProbe {
    fn run(&self, cmd: &Path, args: &[&str]) -> RunResult {
        match std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(out) => RunResult {
                ok: out.status.success(),
                stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            },
            Err(_) => RunResult {
                ok: false,
                stdout: String::new(),
            },
        }
    }

    fn read_file(&self, path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }
}

pub fn detect_host_problem(platform: &Platform, probe: &dyn HostProbe) -> Option<String> {
    if platform.os == "macos" {
        let r = probe.run(Path::new("sw_vers"), &["-productVersion"]);
        if !r.ok {
            return None;
        }
        let version = r.stdout.trim().to_string();
        let major: u32 = version.split('.').next()?.parse().ok()?;
        if major < 13 {
            return Some(format!("macOS {version} is older than 13.0"));
        }
        return None;
    }
    if platform.os == "linux" && platform.arch == "x64" {
        let cpuinfo = probe.read_file("/proc/cpuinfo")?;
        if !cpuinfo.contains("sse4_2") {
            return Some(
                "CPU lacks SSE4.2 (bun needs Intel Nehalem or AMD Bulldozer or newer)".to_string(),
            );
        }
    }
    None
}

pub struct DepsEnv {
    pub path: String,
    pub home: PathBuf,
    pub bun_install: Option<PathBuf>,
}

impl DepsEnv {
    pub fn from_process() -> Self {
        Self {
            path: std::env::var("PATH").unwrap_or_default(),
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default(),
            bun_install: std::env::var_os("BUN_INSTALL").map(PathBuf::from),
        }
    }

    fn bun_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(root) = &self.bun_install {
            dirs.push(root.join("bin"));
        }
        dirs.push(self.home.join(".bun").join("bin"));
        dirs
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn find_executable(name: &str, path_var: &str, extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .chain(extra_dirs.iter().cloned())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

pub fn locate_bun(env: &DepsEnv) -> Option<PathBuf> {
    find_executable("bun", &env.path, &env.bun_dirs())
}

pub struct Report {
    pub facts: Facts,
    pub bun_path: Option<PathBuf>,
    pub bun_version: Option<String>,
}

pub fn check_web_deps(
    env: &DepsEnv,
    platform: Platform,
    need_terminal: bool,
    probe: &dyn HostProbe,
) -> Report {
    let found = find_executable("bun", &env.path, &env.bun_dirs());
    let mut bun = BunState::Missing;
    let mut bun_version = None;
    let mut bun_path = None;
    if let Some(path) = found {
        let r = probe.run(&path, &["--version"]);
        if r.ok {
            bun = BunState::Ok;
            bun_version = Some(r.stdout.trim().to_string());
            bun_path = Some(path);
        } else {
            bun = BunState::Broken;
        }
    }
    let tools = KNOWN_TOOLS
        .iter()
        .filter(|t| find_executable(t, &env.path, &[]).is_some())
        .map(|t| t.to_string())
        .collect();
    let ttyd = need_terminal && find_executable("ttyd", &env.path, &[]).is_some();
    let host_problem = detect_host_problem(&platform, probe);
    Report {
        facts: Facts {
            platform,
            bun,
            ttyd,
            need_terminal,
            host_problem,
            tools,
        },
        bun_path,
        bun_version,
    }
}

fn pm_install(platform: &Platform, tools: &[String], pkg: &str) -> String {
    let has = |t: &str| tools.iter().any(|x| x == t);
    if platform.os == "macos" {
        return if has("brew") {
            format!("brew install {pkg}")
        } else {
            format!("Install {pkg} from its official site, then run this again")
        };
    }
    match PACKAGE_MANAGERS.iter().find(|m| has(m)).copied() {
        Some("dnf") => format!("sudo dnf install {pkg}"),
        Some("yum") => format!("sudo yum install {pkg}"),
        Some("pacman") => format!("sudo pacman -S {pkg}"),
        Some("apk") => format!("apk add {pkg}"),
        Some("zypper") => format!("sudo zypper install {pkg}"),
        _ => format!("sudo apt install {pkg}"),
    }
}

fn build_script(platform: &Platform, auto_install: &[String]) -> Script {
    let label = platform_label(platform);
    let install_bun = auto_install.iter().any(|d| d == "bun");
    let install_ttyd = auto_install.iter().any(|d| d == "ttyd");

    if platform.os == "windows" {
        let lines = [
            format!("# tandem-web dependency installer for {label}"),
            format!("# Source: {BUN_DOCS_URL}"),
            "$ErrorActionPreference = 'Stop'".to_string(),
            String::new(),
            "Write-Host '==> Installing bun'".to_string(),
            BUN_INSTALL_PS.to_string(),
            "& \"$env:USERPROFILE\\.bun\\bin\\bun\" --version".to_string(),
            String::new(),
            "Write-Host '==> Done'".to_string(),
            "Write-Host 'If bun is not found in a new terminal, add %USERPROFILE%\\.bun\\bin to your user PATH.'".to_string(),
            String::new(),
        ];
        return Script {
            name: "install-web-deps.ps1".to_string(),
            content: lines.join("\n"),
        };
    }

    let mut lines: Vec<String> = vec![
        "#!/usr/bin/env bash".into(),
        format!("# tandem-web dependency installer for {label}"),
        format!("# Source: {BUN_DOCS_URL}"),
        "set -euo pipefail".into(),
        String::new(),
    ];
    if install_bun {
        lines.extend([
            "echo \"==> Installing bun\"".to_string(),
            BUN_INSTALL_CMD.to_string(),
            "export BUN_INSTALL=\"${BUN_INSTALL:-$HOME/.bun}\"".to_string(),
            "export PATH=\"$BUN_INSTALL/bin:$PATH\"".to_string(),
            "bun --version".to_string(),
            String::new(),
        ]);
    }
    if install_ttyd {
        lines.extend([
            "echo \"==> Installing ttyd\"".to_string(),
            "brew install ttyd".to_string(),
            "ttyd --version".to_string(),
            String::new(),
        ]);
    }
    lines.push("echo \"==> Done\"".into());
    if install_bun {
        lines.extend([
            "echo \"If bun is not found in a new shell, add these lines to your shell profile:\""
                .to_string(),
            "echo '  export BUN_INSTALL=\"$HOME/.bun\"'".to_string(),
            "echo '  export PATH=\"$BUN_INSTALL/bin:$PATH\"'".to_string(),
        ]);
    }
    lines.push(String::new());
    Script {
        name: "install-web-deps.sh".to_string(),
        content: lines.join("\n"),
    }
}

pub fn plan_from_facts(facts: &Facts) -> Plan {
    let platform = &facts.platform;
    let has = |t: &str| facts.tools.iter().any(|x| x == t);
    let bun_missing = facts.bun != BunState::Ok;
    let bun_broken = facts.bun == BunState::Broken;
    let ttyd_missing = facts.need_terminal && !facts.ttyd;

    let mut unsupported_reason = None;
    if bun_missing {
        let os_ok = matches!(platform.os.as_str(), "macos" | "linux" | "windows");
        let arch_ok = matches!(platform.arch.as_str(), "x64" | "arm64");
        if !os_ok || !arch_ok {
            unsupported_reason = Some(format!(
                "no official bun build for {} {}",
                platform.os, platform.arch
            ));
        } else if let Some(problem) = &facts.host_problem {
            unsupported_reason = Some(problem.clone());
        }
    }
    let supported = unsupported_reason.is_none();

    let unix = platform.os == "macos" || platform.os == "linux";
    let mut prerequisites: Vec<&str> = Vec::new();
    if bun_missing && supported && unix {
        if !has("curl") {
            prerequisites.push("curl");
        }
        if platform.os == "linux" && !has("unzip") {
            prerequisites.push("unzip");
        }
    }

    let mut auto_install: Vec<String> = Vec::new();
    if bun_missing && supported && prerequisites.is_empty() {
        auto_install.push("bun".into());
    }
    if ttyd_missing && platform.os == "macos" && has("brew") {
        auto_install.push("ttyd".into());
    }

    let mut manual: Vec<ManualStep> = Vec::new();
    for dep in &prerequisites {
        manual.push(ManualStep {
            dep: dep.to_string(),
            commands: vec![pm_install(platform, &facts.tools, dep)],
        });
    }
    if bun_missing {
        let commands = if !supported {
            vec![format!(
                "See {BUN_DOCS_URL} ({})",
                unsupported_reason.clone().unwrap_or_default()
            )]
        } else if platform.os == "windows" {
            let mut c = vec![BUN_INSTALL_PS.to_string()];
            if has("scoop") {
                c.push("scoop install bun".into());
            }
            c
        } else {
            let mut c = vec![BUN_INSTALL_CMD.to_string()];
            if has("brew") {
                c.push("brew install oven-sh/bun/bun".into());
            }
            c
        };
        manual.push(ManualStep {
            dep: "bun".into(),
            commands,
        });
    }
    if ttyd_missing {
        let commands = match platform.os.as_str() {
            "macos" => {
                if has("brew") {
                    vec!["brew install ttyd".to_string()]
                } else {
                    vec![
                        "Install Homebrew from https://brew.sh, then run: brew install ttyd"
                            .to_string(),
                    ]
                }
            }
            "linux" => vec![pm_install(platform, &facts.tools, "ttyd")],
            _ => {
                if has("scoop") {
                    vec!["scoop install ttyd".to_string()]
                } else {
                    vec!["See https://github.com/tsl0922/ttyd".to_string()]
                }
            }
        };
        manual.push(ManualStep {
            dep: "ttyd".into(),
            commands,
        });
    }

    let script = if auto_install.is_empty() {
        None
    } else {
        Some(build_script(platform, &auto_install))
    };
    Plan {
        supported,
        unsupported_reason,
        bun_missing,
        bun_broken,
        ttyd_missing,
        can_auto_install: !auto_install.is_empty(),
        auto_install,
        manual,
        script,
    }
}

pub fn manual_lines(plan: &Plan, script_path: Option<&Path>) -> Vec<String> {
    let mut lines = vec!["Install the missing dependencies yourself, then try again:".to_string()];
    for step in &plan.manual {
        lines.push(format!("{}:", step.dep));
        for command in &step.commands {
            lines.push(format!("  {command}"));
        }
    }
    if let Some(path) = script_path {
        let runner = if path.extension().and_then(|e| e.to_str()) == Some("ps1") {
            "powershell -File"
        } else {
            "bash"
        };
        lines.push(format!(
            "A script with the same steps is saved at {} (review it, then run: {runner} {}).",
            path.display(),
            path.display()
        ));
    }
    lines
}

pub struct InstallOutcome {
    pub success: bool,
    pub message: String,
}

pub trait DepsRuntime: Send + Sync {
    fn check(&self, need_terminal: bool) -> Report;
    fn write_script(&self, state_dir: &Path, script: &Script) -> io::Result<PathBuf>;
    fn start_install(&self, script_path: &Path, log_path: &Path) -> Receiver<InstallOutcome>;
}

pub struct SystemDepsRuntime;

impl DepsRuntime for SystemDepsRuntime {
    fn check(&self, need_terminal: bool) -> Report {
        check_web_deps(
            &DepsEnv::from_process(),
            detect_platform(),
            need_terminal,
            &SystemProbe,
        )
    }

    fn write_script(&self, state_dir: &Path, script: &Script) -> io::Result<PathBuf> {
        fs::create_dir_all(state_dir)?;
        let path = state_dir.join(&script.name);
        fs::write(&path, &script.content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(path)
    }

    fn start_install(&self, script_path: &Path, log_path: &Path) -> Receiver<InstallOutcome> {
        let (tx, rx) = mpsc::channel();
        let script = script_path.to_path_buf();
        let log = log_path.to_path_buf();
        std::thread::spawn(move || {
            let outcome = run_installer(&script, &log);
            let _ = tx.send(outcome);
        });
        rx
    }
}

fn run_installer(script: &Path, log: &Path) -> InstallOutcome {
    let log_file = match fs::File::create(log) {
        Ok(f) => f,
        Err(e) => {
            return InstallOutcome {
                success: false,
                message: format!("could not create the install log {}: {e}", log.display()),
            };
        }
    };
    let log_err = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            return InstallOutcome {
                success: false,
                message: format!("could not open the install log: {e}"),
            };
        }
    };

    let mut command = if script.extension().and_then(|e| e.to_str()) == Some("ps1") {
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script);
        c
    } else {
        std::process::Command::new(script)
    };
    let status = command
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(log_err)
        .status();
    match status {
        Ok(s) if s.success() => InstallOutcome {
            success: true,
            message: String::new(),
        },
        Ok(s) => InstallOutcome {
            success: false,
            message: format!(
                "the installer script failed (exit status {}); see {}",
                s.code().map_or("unknown".to_string(), |c| c.to_string()),
                log.display()
            ),
        },
        Err(e) => InstallOutcome {
            success: false,
            message: format!("could not run {}: {e}", script.display()),
        },
    }
}
