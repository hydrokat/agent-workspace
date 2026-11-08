#![allow(dead_code)]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use tandem::web_deps::{BunState, DepsRuntime, Facts, InstallOutcome, Platform, Report, Script};

pub struct ReadyDeps;

impl DepsRuntime for ReadyDeps {
    fn check(&self, need_terminal: bool) -> Report {
        Report {
            facts: Facts {
                platform: Platform {
                    os: "macos".into(),
                    arch: "arm64".into(),
                    libc: None,
                },
                bun: BunState::Ok,
                ttyd: need_terminal,
                need_terminal,
                host_problem: None,
                tools: vec!["curl".into()],
            },
            bun_path: Some(PathBuf::from("/fake/bun")),
            bun_version: Some("1.3.3".into()),
        }
    }

    fn write_script(&self, _: &Path, _: &Script) -> io::Result<PathBuf> {
        panic!("a host with bun installed must never generate an installer script");
    }

    fn start_install(&self, _: &Path, _: &Path) -> Receiver<InstallOutcome> {
        panic!("a host with bun installed must never run an installer");
    }
}
