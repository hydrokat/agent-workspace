use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use tandem::commands::{collect_snapshot_json, snapshot_fingerprint, snapshot_watch};
use tandem::config::Config;

fn temp_workspace(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("tandem-test-watch-{name}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(path.join(".agent-workspace/coordination/queries")).unwrap();
    fs::create_dir_all(path.join("specs")).unwrap();
    path
}

#[test]
fn watch_prints_first_snapshot_immediately() {
    let ws = temp_workspace("first-snapshot");
    let config = Config::default();
    let mut out = Vec::new();

    snapshot_watch(&ws, &config, Duration::from_millis(1), &mut out, Some(1)).unwrap();

    let content = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "Expected exactly one line, got: {content:?}"
    );

    let val: serde_json::Value = serde_json::from_str(lines[0]).expect("Must parse as JSON");
    assert!(
        val.get("workspace_id").is_some(),
        "Must contain workspace_id key, got: {val}"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn watch_skips_unchanged_ticks() {
    let ws = temp_workspace("unchanged");
    let config = Config::default();
    let mut out = Vec::new();

    snapshot_watch(&ws, &config, Duration::from_millis(1), &mut out, Some(3)).unwrap();

    let content = String::from_utf8(out).unwrap();
    let payload_lines: Vec<&str> = content
        .lines()
        .filter(|l| l.contains("workspace_id"))
        .collect();

    assert_eq!(
        payload_lines.len(),
        1,
        "Expected exactly one payload line for unchanged workspace, got: {content:?}"
    );

    let _ = fs::remove_dir_all(&ws);
}

struct ChangeInjectingWriter<'a> {
    inner: &'a mut Vec<u8>,
    newlines_seen: usize,
    inject_at_newline: usize,
    inject_path: PathBuf,
    inject_content: Vec<u8>,
    injected: bool,
}

impl Write for ChangeInjectingWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.inner.extend_from_slice(data);
        for &b in data {
            if b == b'\n' {
                self.newlines_seen += 1;
                if self.newlines_seen >= self.inject_at_newline && !self.injected {
                    self.injected = true;
                    let _ = fs::create_dir_all(self.inject_path.parent().unwrap());
                    let _ = fs::write(&self.inject_path, &self.inject_content);
                }
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn watch_emits_on_change() {
    let ws = temp_workspace("on-change");
    let config = Config::default();
    let mut raw = Vec::new();

    let impl_path = ws.join("specs/p-001-test/impl.md");

    let mut writer = ChangeInjectingWriter {
        inner: &mut raw,
        newlines_seen: 0,
        inject_at_newline: 1,
        inject_path: impl_path,
        inject_content: b"# test\n## Tasks\n- [ ] t-001: task\n".to_vec(),
        injected: false,
    };

    snapshot_watch(&ws, &config, Duration::from_millis(1), &mut writer, Some(5)).unwrap();

    let content = String::from_utf8(raw).unwrap();
    let payload_lines: Vec<&str> = content
        .lines()
        .filter(|l| l.contains("workspace_id"))
        .collect();

    assert!(
        payload_lines.len() >= 2,
        "Expected at least 2 payload lines after file change, got {}: {content:?}",
        payload_lines.len()
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn fingerprint_ignores_now() {
    let ws = temp_workspace("fingerprint");
    let config = Config::default();

    let p1 = collect_snapshot_json(&ws, &config).unwrap();
    let mut p2 = p1.clone();
    p2.snapshot.now = p1.snapshot.now + 100.0;

    assert_eq!(
        snapshot_fingerprint(&p1),
        snapshot_fingerprint(&p2),
        "Fingerprints should be equal when only snapshot.now differs"
    );

    let mut p3 = p1.clone();
    p3.workspace_id = "tandem-different-workspace-id".to_string();

    assert_ne!(
        snapshot_fingerprint(&p1),
        snapshot_fingerprint(&p3),
        "Fingerprints should differ when workspace_id changes"
    );

    let _ = fs::remove_dir_all(&ws);
}

struct BrokenPipeWriter;

impl Write for BrokenPipeWriter {
    fn write(&mut self, _data: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn watch_stops_when_writer_fails() {
    let ws = temp_workspace("broken-pipe");
    let config = Config::default();

    let mut writer = BrokenPipeWriter;
    let result = snapshot_watch(&ws, &config, Duration::from_millis(1), &mut writer, Some(5));

    assert!(result.is_err(), "Should return Err when writer fails");

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn watch_line_has_no_newlines_inside() {
    let ws = temp_workspace("no-inner-newlines");
    let config = Config::default();
    let mut out = Vec::new();

    snapshot_watch(&ws, &config, Duration::from_millis(1), &mut out, Some(1)).unwrap();

    let content = String::from_utf8(out).unwrap();
    assert!(content.ends_with('\n'), "Output should end with newline");
    let trimmed = content.trim_end_matches('\n');
    assert!(
        !trimmed.contains('\n'),
        "Line should contain no internal newlines, got: {trimmed:?}"
    );

    let _ = fs::remove_dir_all(&ws);
}
