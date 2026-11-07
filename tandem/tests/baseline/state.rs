use std::fs;

use tandem::state::{parse, save};

use crate::common::{TempDir, assert_golden};

#[test]
fn test_golden_state_v1_and_v2_round_trip() {
    let tmp = TempDir::new();

    // 1. v1 input
    let v1_json = serde_json::json!({
        "version": 1,
        "agents": [
            {
                "cli": "claude",
                "role": "planner",
                "model": "opus",
                "index": 1,
                "skip_permissions": false,
                "cwd": "/mock/workspace"
            },
            {
                "cli": "agy",
                "role": "implementer",
                "model": "gemini-3.8-flash-high",
                "index": 2,
                "skip_permissions": true
            }
        ],
        "active": {
            "planner": 1,
            "implementer": 2
        },
        "layout": {
            "columns_percent": 45,
            "status_rows": 8
        },
        "saved_at": 1_700_000_000.0,
        "clean": true
    });

    let parsed_v1 = parse(&v1_json).expect("parse v1");
    let v1_path = tmp.path().join("v1_saved.json");
    save(&v1_path, &parsed_v1).expect("save v1");
    let v1_saved_content = fs::read_to_string(&v1_path).unwrap();
    assert_golden("state/v1_roundtrip.json", &v1_saved_content);

    // 2. v2 input with conversation ids
    let v2_json = serde_json::json!({
        "version": 2,
        "agents": [
            {
                "cli": "claude",
                "role": "planner",
                "model": "sonnet",
                "index": 1,
                "skip_permissions": false,
                "conversation": "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"
            },
            {
                "cli": "agy",
                "role": "implementer",
                "model": "gemini-3.8-flash-high",
                "index": 1,
                "skip_permissions": true,
                "conversation": "7a9b8c0d-1e2f-3a4b-5c6d-7e8f9a0b1c2d"
            }
        ],
        "active": {
            "planner": 1,
            "implementer": 1
        },
        "layout": {
            "columns_percent": 50,
            "status_rows": 10
        },
        "saved_at": 1_700_000_100.0,
        "clean": true
    });

    let parsed_v2 = parse(&v2_json).expect("parse v2");
    let v2_path = tmp.path().join("v2_saved.json");
    save(&v2_path, &parsed_v2).expect("save v2");
    let v2_saved_content = fs::read_to_string(&v2_path).unwrap();
    assert_golden("state/v2_roundtrip.json", &v2_saved_content);
}

#[test]
fn test_golden_state_v3_round_trip() {
    let tmp = TempDir::new();

    let v3_json = serde_json::json!({
        "version": 3,
        "agents": [
            {
                "cli": "claude",
                "role": "planner",
                "model": "sonnet",
                "index": 1,
                "skip_permissions": false,
                "conversation": "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"
            },
            {
                "cli": "agy",
                "role": "implementer",
                "model": "gemini-3.8-flash-high",
                "index": 1,
                "skip_permissions": true,
                "conversation": "7a9b8c0d-1e2f-3a4b-5c6d-7e8f9a0b1c2d"
            }
        ],
        "active": {
            "planner": 1,
            "implementer": 1
        },
        "ui": {
            "sidebar_cols": 26,
            "status_rows": 10,
            "selected": ["46f2fc12-7f1f-4ca2-b714-06cbbfd669e9", "7a9b8c0d-1e2f-3a4b-5c6d-7e8f9a0b1c2d"]
        },
        "saved_at": 1_700_000_200.0,
        "clean": true
    });

    let parsed_v3 = parse(&v3_json).expect("parse v3");
    assert_eq!(parsed_v3.version, 3);
    assert_eq!(parsed_v3.ui.sidebar_cols, 26);
    assert_eq!(parsed_v3.ui.status_rows, 10);
    assert_eq!(parsed_v3.ui.selected.len(), 2);

    let v3_path = tmp.path().join("v3_saved.json");
    save(&v3_path, &parsed_v3).expect("save v3");
    let v3_saved_content = fs::read_to_string(&v3_path).unwrap();
    assert_golden("state/v3_roundtrip.json", &v3_saved_content);
}
