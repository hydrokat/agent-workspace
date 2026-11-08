#![allow(dead_code)]

use tandem::tmux::client::TmuxClient;

pub fn capture_topology(client: &TmuxClient) -> String {
    let format = "#{window_name}\t#{@tandem_role}\t#{@tandem_kind}\t#{@tandem_index}\t#{@tandem_label}\t#{pane_dead}\t#{pane_width}x#{pane_height}+#{pane_left}+#{pane_top}";
    let out = client
        .run(&["list-panes", "-a", "-F", format])
        .map(|r| r.out)
        .unwrap_or_default();

    let mut lines: Vec<String> = out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    lines.sort();
    lines.join("\n")
}
