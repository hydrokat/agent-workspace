mod common;

use common::parity::load_fixture_value;
use std::path::Path;
use tandem::coordination::schema::Claim;
use tandem::sources::conflicts::find_conflicts;
use tandem::sources::inprogress::parse_in_progress;
use tandem::sources::specs::load_phases;

const IN_PROGRESS_MD: &str = r#"# In Progress

## Example

### p-002-payments-api — t-003: Add refresh token rotation
- **Agent:** `@senior-backend-dev` (ID: `abc-1234`)

### p-001-thing — t-002: Real work
- **Agent:** `@senior-backend-dev` (ID: `sess-1`)
- **Location:** Branch `feat/x`
- **Ongoing:** Writing code.
- **Next step:** Test.
"#;

#[test]
fn test_sources_parity_fixture() {
    let fixture = load_fixture_value("sources");

    let claims = vec![
        Claim {
            phase: "p-001".to_string(),
            task: "t-001".to_string(),
            session: "sess-1".to_string(),
            branch: String::new(),
            worktree: String::new(),
            paths: vec!["src/main.rs".to_string()],
            heartbeat: 100.0,
        },
        Claim {
            phase: "p-001".to_string(),
            task: "t-002".to_string(),
            session: "sess-2".to_string(),
            branch: String::new(),
            worktree: String::new(),
            paths: vec!["src/main.rs".to_string()],
            heartbeat: 100.0,
        },
    ];
    let conflicts = find_conflicts(&claims);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].first, "t-001");
    assert_eq!(conflicts[0].second, "t-002");
    assert_eq!(conflicts[0].path, "src/main.rs");

    let expected_conflicts = fixture["conflicts"].as_array().unwrap();
    assert_eq!(expected_conflicts.len(), 1);
    assert_eq!(expected_conflicts[0]["first"], "t-001");
    assert_eq!(expected_conflicts[0]["second"], "t-002");
    assert_eq!(expected_conflicts[0]["path"], "src/main.rs");

    let temp = common::TempDir::new();
    let in_progress_file = temp.path().join("in-progress.md");
    std::fs::write(&in_progress_file, IN_PROGRESS_MD).unwrap();

    let parsed_inprogress = parse_in_progress(&in_progress_file);
    let serialized_inprogress = serde_json::to_value(&parsed_inprogress).unwrap();
    assert_eq!(serialized_inprogress, fixture["inprogress"]);

    let specs_fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test")
        .join("skills")
        .join("fixtures")
        .join("specs");

    let phases = load_phases(&specs_fixture_dir);
    let expected_phases = fixture["phases"].as_array().unwrap();
    assert_eq!(phases.len(), expected_phases.len());

    for (p, exp) in phases.iter().zip(expected_phases.iter()) {
        assert_eq!(p.name, exp["name"].as_str().unwrap());
        assert_eq!(p.done(), exp["done"].as_u64().unwrap() as usize);
        assert_eq!(p.total(), exp["total"].as_u64().unwrap() as usize);
        assert_eq!(p.warning, exp["warning"].as_str().unwrap());

        let exp_tasks = exp["tasks"].as_array().unwrap();
        assert_eq!(p.tasks.len(), exp_tasks.len());
        for (t, exp_t) in p.tasks.iter().zip(exp_tasks.iter()) {
            assert_eq!(t.id, exp_t["id"].as_str().unwrap());
            assert_eq!(t.title, exp_t["title"].as_str().unwrap());
            assert_eq!(t.status, exp_t["status"].as_str().unwrap());
        }
    }

    let store = tandem::coordination::store::CoordStore::new(specs_fixture_dir.join(".agents"));
    let snap = tandem::snapshot::collect(
        &specs_fixture_dir,
        &store,
        std::time::Duration::from_secs(1),
    );
    assert_eq!(snap.phases.len(), expected_phases.len());
    assert_eq!(snap.entries.len(), 0);
    assert_eq!(snap.conflicts.len(), 0);
    assert_eq!(snap.claims.len(), 0);
    assert_eq!(snap.open_queries(), 0);
}
