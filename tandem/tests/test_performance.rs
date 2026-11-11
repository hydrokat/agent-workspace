mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tandem::coordination::store::CoordStore;
use tandem::snapshot::collect;
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::layout::{Launch, Layout};

struct CountingRunner {
    calls: Mutex<Vec<Vec<String>>>,
    panes_output: Mutex<String>,
}

impl CountingRunner {
    fn new(panes: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            panes_output: Mutex::new(panes.to_string()),
        }
    }

    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn reset(&self) {
        self.calls.lock().unwrap().clear();
    }
}

impl TmuxRunner for CountingRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        let joined = argv.join(" ");

        if joined.contains("list-panes") {
            return TmuxResult {
                code: 0,
                out: self.panes_output.lock().unwrap().clone(),
                err: String::new(),
            };
        }
        if joined.contains("new-session") {
            return TmuxResult {
                code: 0,
                out: "%1\n".to_string(),
                err: String::new(),
            };
        }
        if joined.contains("split-window") {
            let id = format!("%{}\n", self.count() + 1);
            return TmuxResult {
                code: 0,
                out: id,
                err: String::new(),
            };
        }
        if joined.contains("display-message") && joined.contains("#{pane_height}") {
            return TmuxResult {
                code: 0,
                out: "10\n".to_string(),
                err: String::new(),
            };
        }

        TmuxResult {
            code: 0,
            out: String::new(),
            err: String::new(),
        }
    }
}

fn sample_panes_tsv() -> String {
    let p1 = "%1\tmain\town\tsidebar\t0\tsidebar\t0\t\t\t\t";
    let p2 = "%2\tmain\tagent\tplanner\t1\tclaude·opus\t0\t\t\t\t";
    let p3 = "%3\tmain\town\tstatus\t0\t\t0\t\t\t\t";
    format!("{p1}\n{p2}\n{p3}\n")
}

#[test]
fn test_measure_baseline_spawn_counts() {
    let runner = Arc::new(CountingRunner::new(&sample_panes_tsv()));
    let client = TmuxClient::with_runner(
        "tandem-perf",
        Some(PathBuf::from("/mock/tandem.conf")),
        runner.clone(),
    );
    let mut layout = Layout::new(&client);
    layout.workspace = Some("/mock/workspace".to_string());

    let mut sidebar = Launch::new(
        vec!["tandem".to_string(), "sidebar".to_string()],
        "sidebar",
        0,
        "sidebar",
    );
    sidebar.kind = "own".to_string();

    let main = Launch::new(vec!["claude".to_string()], "planner", 1, "claude·opus");

    let mut status = Launch::new(
        vec!["tandem".to_string(), "status".to_string()],
        "status",
        0,
        "status",
    );
    status.kind = "own".to_string();

    // 1. Measure build
    runner.reset();
    let _ = layout.build(120, 40, &sidebar, &main, &status);
    let build_spawns = runner.count();

    // 2. Measure add_agent
    runner.reset();
    let new_agent = Launch::new(vec!["claude".to_string()], "implementer", 1, "claude·haiku");
    let _ = layout.add_agent(&new_agent, true);
    let add_agent_spawns = runner.count();

    // 3. Measure step (tab cycle)
    runner.reset();
    let _ = layout.step("planner", 1);
    let step_spawns = runner.count();

    // 4. Measure close_agent
    runner.reset();
    let placeholder = Launch::new(vec!["tandem".to_string()], "implementer", 0, "placeholder");
    let _ = layout.close_agent("%2", &placeholder);
    let close_agent_spawns = runner.count();

    // 5. Measure refresh
    runner.reset();
    let _ = layout.refresh();
    let refresh_spawns = runner.count();

    // 6. Measure reflow
    runner.reset();
    let _ = layout.reflow(120, 40);
    let reflow_spawns = runner.count();

    println!("\n=== BASELINE SPAWN COUNTS ===");
    println!("build:       {build_spawns}");
    println!("add_agent:   {add_agent_spawns}");
    println!("step:        {step_spawns}");
    println!("close_agent: {close_agent_spawns}");
    println!("refresh:     {refresh_spawns}");
    println!("reflow:      {reflow_spawns}");

    tandem::sources::gitinfo::clear_git_info_cache();
    let store = CoordStore::new(PathBuf::from("/tmp"));
    let repo_root = Path::new("..");
    let start_cold = Instant::now();
    let _ = collect(repo_root, &store, Duration::from_millis(1000));
    let cold_duration = start_cold.elapsed();

    let start_warm = Instant::now();
    let _ = collect(repo_root, &store, Duration::from_millis(1000));
    let warm_duration = start_warm.elapsed();

    println!("collect() cold time: {:?}", cold_duration);
    println!("collect() warm time: {:?}", warm_duration);

    assert!(
        build_spawns <= 10,
        "build spawned {build_spawns} (expected <= 10)"
    );
    assert!(
        add_agent_spawns <= 6,
        "add_agent spawned {add_agent_spawns} (expected <= 6)"
    );
    assert!(
        step_spawns <= 1,
        "step spawned {step_spawns} (expected <= 1)"
    );
    assert!(
        close_agent_spawns <= 5,
        "close_agent spawned {close_agent_spawns} (expected <= 5)"
    );
    assert!(
        refresh_spawns <= 2,
        "refresh spawned {refresh_spawns} (expected <= 2)"
    );
    assert!(
        reflow_spawns <= 4,
        "reflow spawned {reflow_spawns} (expected <= 4)"
    );
    assert!(
        warm_duration < cold_duration,
        "warm collect should be faster than cold"
    );
}

#[test]
fn test_snapshot_at_most_two_tmux_calls() {
    let runner = Arc::new(CountingRunner::new(&sample_panes_tsv()));
    let client = TmuxClient::with_runner(
        "tandem-snap-test",
        Some(PathBuf::from("/mock/tandem.conf")),
        runner.clone(),
    );
    let layout = Layout::new(&client);

    runner.reset();
    let panes = layout.panes().unwrap_or_default();
    let _ = layout.selected_and_size(&panes);
    let call_count = runner.count();

    assert!(
        call_count <= 2,
        "snapshot path should make at most 2 tmux calls, made {call_count}"
    );

    let calls = runner.calls.lock().unwrap();
    for call in calls.iter() {
        assert!(
            !call.iter().any(|arg| arg == "has-session"),
            "has-session must not appear in the snapshot tmux call path; calls: {calls:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn test_git_cache_between_calls() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = common::TempDir::new();
    let counter_file = tmp.path().join("git-call-count");
    std::fs::write(&counter_file, "0").unwrap();

    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git/refs/heads")).unwrap();
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(repo.join(".git/index"), []).unwrap();

    let counter_path = counter_file.to_str().unwrap();
    let repo_path = repo.to_str().unwrap();
    let script = format!(
        "#!/bin/sh\n\
         cnt=$(cat '{c}' 2>/dev/null || echo 0)\n\
         printf '%d' $((cnt + 1)) > '{c}'\n\
         case \"$*\" in\n\
         *show-current*)   echo 'main' ;;\n\
         *for-each-ref*)   echo 'main' ;;\n\
         *'worktree list'*) printf 'worktree {r}\\nbranch refs/heads/main\\n' ;;\n\
         esac\n",
        c = counter_path,
        r = repo_path
    );
    let script_path = tmp.path().join("fake-git");
    std::fs::write(&script_path, script.as_bytes()).unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();

    tandem::sources::gitinfo::clear_git_info_cache();
    let script_str = script_path.to_str().unwrap();

    let r1 = tandem::sources::gitinfo::git_info_with_git(&repo, Duration::from_secs(5), script_str);
    let r2 = tandem::sources::gitinfo::git_info_with_git(&repo, Duration::from_secs(5), script_str);

    assert!(r1.is_some(), "First call should succeed");
    assert!(
        r2.is_some(),
        "Second call should succeed (served from cache)"
    );

    let count_str = std::fs::read_to_string(&counter_file).unwrap_or_default();
    let count: u32 = count_str.trim().parse().unwrap_or(0);
    assert_eq!(
        count, 3,
        "Git binary should run exactly 3 times on first call and 0 times on second (cache hit), got {count}"
    );
}
