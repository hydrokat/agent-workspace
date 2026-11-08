use sha2::{Digest, Sha512};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908b0df;
const UPPER_MASK: u32 = 0x80000000;
const LOWER_MASK: u32 = 0x7fffffff;

pub const ALLOWED: &[&str] = &[
    "new-session",
    "new-window",
    "split-window",
    "swap-pane",
    "select-pane",
    "resize-pane",
    "kill-pane",
    "move-pane",
    "join-pane",
    "break-pane",
    "respawn-pane",
    "select-layout",
    "select-window",
    "set-option",
    "set-window-option",
    "set-hook",
    "bind-key",
    "unbind-key",
    "display-message",
    "display-popup",
    "display-menu",
    "confirm-before",
    "list-panes",
    "list-windows",
    "has-session",
    "kill-session",
    "attach-session",
    "detach-client",
    "switch-client",
    "source-file",
    "show-options",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Result {
    pub code: i32,
    pub out: String,
    pub err: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TmuxError {
    #[error("tmux {cmd} failed: {err}")]
    Failed { cmd: String, err: String },
    #[error("ToS violation: {0}")]
    ToS(#[from] ToSViolation),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ToSViolation {
    #[error("tmux subcommand not allowed: {0}")]
    SubcommandNotAllowed(String),
    #[error("refusing to touch a pane that is not tandem-owned")]
    NotTandemOwned,
    #[error("invalid pane id")]
    InvalidPaneId,
}

pub trait TmuxRunner: Send + Sync {
    fn run(&self, argv: &[String]) -> Result;
}

pub struct SubprocessRunner;

impl TmuxRunner for SubprocessRunner {
    fn run(&self, argv: &[String]) -> Result {
        if argv.is_empty() {
            return Result {
                code: 1,
                out: String::new(),
                err: "empty argv".to_string(),
            };
        }
        match std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(out) => Result {
                code: out.status.code().unwrap_or(-1),
                out: String::from_utf8_lossy(&out.stdout).into_owned(),
                err: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
            Err(e) => Result {
                code: 1,
                out: String::new(),
                err: e.to_string(),
            },
        }
    }
}

impl<T: TmuxRunner + ?Sized> TmuxRunner for Arc<T> {
    fn run(&self, argv: &[String]) -> Result {
        (**self).run(argv)
    }
}

#[derive(Clone)]
pub struct TmuxClient {
    pub socket: String,
    pub conf: Option<PathBuf>,
    runner: Arc<dyn TmuxRunner>,
}

impl std::fmt::Debug for TmuxClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TmuxClient")
            .field("socket", &self.socket)
            .field("conf", &self.conf)
            .finish()
    }
}

impl TmuxClient {
    pub fn new(socket: impl Into<String>, conf: Option<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            conf,
            runner: Arc::new(SubprocessRunner),
        }
    }

    pub fn with_runner(
        socket: impl Into<String>,
        conf: Option<PathBuf>,
        runner: Arc<dyn TmuxRunner>,
    ) -> Self {
        Self {
            socket: socket.into(),
            conf,
            runner,
        }
    }

    fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut out = vec!["tmux".to_string(), "-L".to_string(), self.socket.clone()];
        if let Some(ref c) = self.conf {
            out.push("-f".to_string());
            out.push(c.to_string_lossy().into_owned());
        }
        for a in args {
            out.push((*a).to_string());
        }
        out
    }

    pub fn run(&self, args: &[&str]) -> std::result::Result<Result, ToSViolation> {
        if args.is_empty() || !ALLOWED.contains(&args[0]) {
            let cmd = if args.is_empty() { "(none)" } else { args[0] };
            return Err(ToSViolation::SubcommandNotAllowed(cmd.to_string()));
        }
        Ok(self.runner.run(&self.argv(args)))
    }

    pub fn run_checked(&self, args: &[&str]) -> std::result::Result<String, TmuxError> {
        let cmd = if args.is_empty() { "(none)" } else { args[0] };
        let res = self.run(args)?;
        if res.code != 0 {
            return Err(TmuxError::Failed {
                cmd: cmd.to_string(),
                err: res.err.trim().to_string(),
            });
        }
        Ok(res.out)
    }

    pub fn run_batch(&self, commands: &[Vec<String>]) -> std::result::Result<Result, ToSViolation> {
        if commands.is_empty() {
            return Ok(Result {
                code: 0,
                out: String::new(),
                err: String::new(),
            });
        }
        for cmd in commands {
            if cmd.is_empty() || !ALLOWED.contains(&cmd[0].as_str()) {
                let first = if cmd.is_empty() {
                    "(none)"
                } else {
                    cmd[0].as_str()
                };
                return Err(ToSViolation::SubcommandNotAllowed(first.to_string()));
            }
        }
        let mut argv = vec!["tmux".to_string(), "-L".to_string(), self.socket.clone()];
        if let Some(ref c) = self.conf {
            argv.push("-f".to_string());
            argv.push(c.to_string_lossy().into_owned());
        }
        for (i, cmd) in commands.iter().enumerate() {
            if i > 0 {
                argv.push(";".to_string());
            }
            argv.extend_from_slice(cmd);
        }
        Ok(self.runner.run(&argv))
    }

    pub fn run_batch_checked(
        &self,
        commands: &[Vec<String>],
    ) -> std::result::Result<String, TmuxError> {
        if commands.is_empty() {
            return Ok(String::new());
        }
        let res = self.run_batch(commands)?;
        if res.code != 0 {
            return Err(TmuxError::Failed {
                cmd: "batch".to_string(),
                err: res.err.trim().to_string(),
            });
        }
        Ok(res.out)
    }

    pub fn pane_kind(&self, pane_id: &str) -> std::result::Result<String, ToSViolation> {
        if !is_valid_pane_id(pane_id) {
            return Err(ToSViolation::InvalidPaneId);
        }
        let res = self.run(&["display-message", "-p", "-t", pane_id, "#{@tandem_kind}"])?;
        Ok(res.out.trim().to_string())
    }

    fn require_own(&self, pane_id: &str) -> std::result::Result<(), ToSViolation> {
        if self.pane_kind(pane_id)? != "own" {
            return Err(ToSViolation::NotTandemOwned);
        }
        Ok(())
    }

    pub fn send_keys_own(
        &self,
        pane_id: &str,
        keys: &[&str],
    ) -> std::result::Result<(), ToSViolation> {
        self.require_own(pane_id)?;
        let mut args = vec!["send-keys", "-t", pane_id];
        args.extend_from_slice(keys);
        self.runner.run(&self.argv(&args));
        Ok(())
    }

    pub fn capture_own(&self, pane_id: &str) -> std::result::Result<String, ToSViolation> {
        self.require_own(pane_id)?;
        let args = ["capture-pane", "-p", "-t", pane_id];
        let res = self.runner.run(&self.argv(&args));
        Ok(res.out)
    }
}

fn is_valid_pane_id(id: &str) -> bool {
    let digits = match id.strip_prefix('%') {
        Some(d) => d,
        None => return false,
    };
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

struct Mt19937 {
    mt: [u32; N],
    mti: usize,
}

impl Mt19937 {
    fn new(key: &[u32]) -> Self {
        let mut mt = [0u32; N];
        mt[0] = 19650218;
        for i in 1..N {
            let prev = mt[i - 1];
            mt[i] = (1812433253u32.wrapping_mul(prev ^ (prev >> 30))).wrapping_add(i as u32);
        }
        let mut i = 1;
        let mut j = 0;
        let k = N.max(key.len());
        for _ in (1..=k).rev() {
            let prev = mt[i - 1];
            let term = (prev ^ (prev >> 30)).wrapping_mul(1664525);
            mt[i] = (mt[i] ^ term).wrapping_add(key[j]).wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in (1..N).rev() {
            let prev = mt[i - 1];
            let term = (prev ^ (prev >> 30)).wrapping_mul(1566083941);
            mt[i] = (mt[i] ^ term).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
        }
        mt[0] = 0x80000000;
        Self { mt, mti: N }
    }

    fn genrand_int32(&mut self) -> u32 {
        let mag01 = [0u32, MATRIX_A];
        if self.mti >= N {
            for kk in 0..N - M {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            }
            for kk in (N - M)..(N - 1) {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M - N] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            }
            let y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
            self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            self.mti = 0;
        }

        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;
        y
    }

    fn getrandbits_40(&mut self) -> u64 {
        let r1 = self.genrand_int32() as u64;
        let r2 = self.genrand_int32() as u64;
        r1 | ((r2 >> 24) << 32)
    }
}

pub fn resolve_path(path: &Path) -> PathBuf {
    if let Ok(canon) = path.canonicalize() {
        return canon;
    }
    let mut current = path;
    let mut tail = Vec::new();
    while !current.as_os_str().is_empty() && !current.exists() {
        if let Some(name) = current.file_name() {
            tail.push(name);
        }
        if let Some(parent) = current.parent() {
            current = parent;
        } else {
            break;
        }
    }
    tail.reverse();
    let base = if current.exists() {
        current
            .canonicalize()
            .unwrap_or_else(|_| current.to_path_buf())
    } else {
        current.to_path_buf()
    };
    let mut result = base;
    for part in tail {
        result.push(part);
    }
    result
}

pub fn socket_for(workspace: &Path) -> String {
    let resolved = resolve_path(workspace);
    let resolved_str = resolved.to_string_lossy();
    let mut hasher = Sha512::new();
    hasher.update(resolved_str.as_bytes());
    let digest = hasher.finalize();

    let mut data = Vec::with_capacity(resolved_str.len() + 64);
    data.extend_from_slice(resolved_str.as_bytes());
    data.extend_from_slice(&digest);

    let mut key = Vec::new();
    let mut remaining = data.len();
    while remaining >= 4 {
        let chunk = &data[remaining - 4..remaining];
        let val = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        key.push(val);
        remaining -= 4;
    }
    if remaining > 0 {
        let mut buf = [0u8; 4];
        let offset = 4 - remaining;
        buf[offset..].copy_from_slice(&data[..remaining]);
        key.push(u32::from_be_bytes(buf));
    }
    if key.is_empty() {
        key.push(0);
    }

    let mut rng = Mt19937::new(&key);
    format!("tandem-{:010x}", rng.getrandbits_40())
}
