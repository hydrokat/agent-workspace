use std::collections::HashMap;
use std::path::PathBuf;

use tandem::coordination::store::CoordStore;
use tandem::tmux::client::TmuxClient;
use tandem::tmux::layout::{Launch, Layout};
use tandem::tmux::tabs::visible_pane;

use crate::common::{TempDir, assert_golden, capture_topology, require_tool};

struct TestContext {
    socket: String,
    #[allow(dead_code)]
    temp_dir: TempDir,
    layout: Layout,
    #[allow(dead_code)]
    bin_dir: PathBuf,
}

impl TestContext {
    fn new() -> Option<Self> {
        if !require_tool("tmux") {
            return None;
        }
        let temp_dir = TempDir::new();
        let socket = format!(
            "tandem-topo-{}-{}",
            std::process::id(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        for stub in ["claude", "agy"] {
            let stub_path = bin_dir.join(stub);
            std::fs::write(
                &stub_path,
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"1.0.0\"; exit 0; fi\nexec sleep 60\n",
            )
            .expect("write stub");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&stub_path).expect("meta").permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&stub_path, perms).expect("chmod");
            }
        }
        let client = TmuxClient::new(&socket, None);
        let coord = CoordStore::new(temp_dir.path().join("coord"));
        let mut lay = Layout::new(&client);
        lay.coord = Some(coord);
        let conf_path = temp_dir.path().join("tandem.conf");
        let conf = lay.write_conf(&conf_path).expect("write_conf");
        let client_with_conf = TmuxClient::new(&socket, Some(conf));
        lay = Layout::new(&client_with_conf);
        lay.coord = Some(CoordStore::new(temp_dir.path().join("coord")));

        Some(Self {
            socket,
            temp_dir,
            layout: lay,
            bin_dir,
        })
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
    }
}

fn sleeper(role: &str, index: usize, kind: &str, label: &str) -> Launch {
    let lbl = if label.is_empty() {
        format!("fake·{}{}", role, index)
    } else {
        label.to_string()
    };
    let mut env = HashMap::new();
    env.insert(
        "TANDEM_SESSION_ID".to_string(),
        format!("{}-{}", role, index),
    );
    let session_id = if kind == "agent" {
        format!("{}-{}", role, index)
    } else {
        String::new()
    };
    let mut launch = Launch::new(
        vec!["sleep".to_string(), "60".to_string()],
        role,
        index,
        &lbl,
    );
    launch.env = env;
    launch.session_id = session_id;
    launch.kind = kind.to_string();
    launch
}

#[test]
fn test_golden_tmux_topology_lifecycle() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };

    // 1. Build initial layout (160x40)
    ctx.layout
        .build(
            160,
            40,
            &sleeper("planner", 1, "agent", ""),
            &sleeper("implementer", 1, "agent", ""),
            &sleeper("status", 0, "own", ""),
        )
        .expect("build");
    assert_golden(
        "topology/01_build.txt",
        &capture_topology(&ctx.layout.client),
    );

    // 2. Add an implementer agent (index 2)
    ctx.layout
        .add_agent(&sleeper("implementer", 2, "agent", ""), true)
        .expect("add_agent");
    assert_golden(
        "topology/02_add_agent.txt",
        &capture_topology(&ctx.layout.client),
    );

    // 3. Step tab in implementer column
    ctx.layout.step("implementer", 1).expect("step");
    assert_golden(
        "topology/03_step_tab.txt",
        &capture_topology(&ctx.layout.client),
    );

    // 4. Close visible implementer agent
    let panes = ctx.layout.panes().unwrap();
    let imp_visible = visible_pane(&panes, "implementer").expect("visible imp");
    ctx.layout
        .close_agent(
            &imp_visible.id,
            &sleeper("implementer", 0, "placeholder", "Add agent"),
        )
        .expect("close_agent");
    assert_golden(
        "topology/04_close_agent.txt",
        &capture_topology(&ctx.layout.client),
    );

    // 5. Close remaining implementer -> turns into placeholder
    let panes2 = ctx.layout.panes().unwrap();
    let imp_remaining = visible_pane(&panes2, "implementer").expect("visible remaining imp");
    ctx.layout
        .close_agent(
            &imp_remaining.id,
            &sleeper("implementer", 0, "placeholder", "Add agent"),
        )
        .expect("close_last_agent");
    assert_golden(
        "topology/05_placeholder.txt",
        &capture_topology(&ctx.layout.client),
    );

    // 6. Reset layout
    ctx.layout.reset().expect("reset");
    assert_golden(
        "topology/06_reset.txt",
        &capture_topology(&ctx.layout.client),
    );

    // 7. Reflow at 120x30
    ctx.layout.reflow(120, 30).expect("reflow");
    assert_golden(
        "topology/07_reflow.txt",
        &capture_topology(&ctx.layout.client),
    );
}
