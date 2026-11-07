use tandem::coordination::schema::Query;
use tandem::sidebar::model::{
    SidebarRow, first_selectable, last_selectable, next_selectable, prev_selectable, rows_from,
};
use tandem::tmux::tabs::PaneInfo;

#[test]
fn test_sidebar_model_empty_panes() {
    let rows = rows_from(&[], &[], &[], None, &[]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], SidebarRow::Empty);
    assert_eq!(rows[1], SidebarRow::AddAgent);

    assert!(!rows[0].is_selectable());
    assert!(rows[1].is_selectable());
    assert_eq!(first_selectable(&rows), 1);
    assert_eq!(last_selectable(&rows), 1);
}

#[test]
fn test_sidebar_model_groups_and_sorting() {
    let p1 = PaneInfo::new(
        "%1",
        "main",
        "agent",
        "implementer",
        2,
        "claude·sonnet",
        false,
        None,
    );
    let p2 = PaneInfo::new(
        "%2",
        "main",
        "agent",
        "planner",
        1,
        "claude·opus",
        false,
        None,
    );
    let p3 = PaneInfo::new(
        "%3",
        "main",
        "agent",
        "implementer",
        1,
        "agy·flash",
        false,
        None,
    );

    let rows = rows_from(&[p1, p2, p3], &[], &[], None, &[]);

    // Expect: PLANNERS Header, planner 1, IMPLEMENTERS Header, impl 1, impl 2, AddAgent
    assert_eq!(rows.len(), 6);
    assert_eq!(
        rows[0],
        SidebarRow::Header {
            title: "PLANNERS".to_string(),
            count: 1
        }
    );
    match &rows[1] {
        SidebarRow::Agent {
            pane_id,
            role,
            index,
            ..
        } => {
            assert_eq!(pane_id, "%2");
            assert_eq!(role, "planner");
            assert_eq!(*index, 1);
        }
        _ => panic!("expected agent row"),
    }
    assert_eq!(
        rows[2],
        SidebarRow::Header {
            title: "IMPLEMENTERS".to_string(),
            count: 2
        }
    );
    match &rows[3] {
        SidebarRow::Agent {
            pane_id,
            role,
            index,
            ..
        } => {
            assert_eq!(pane_id, "%3");
            assert_eq!(role, "implementer");
            assert_eq!(*index, 1);
        }
        _ => panic!("expected agent row"),
    }
    match &rows[4] {
        SidebarRow::Agent {
            pane_id,
            role,
            index,
            ..
        } => {
            assert_eq!(pane_id, "%1");
            assert_eq!(role, "implementer");
            assert_eq!(*index, 2);
        }
        _ => panic!("expected agent row"),
    }
    assert_eq!(rows[5], SidebarRow::AddAgent);
}

#[test]
fn test_sidebar_model_glyphs() {
    // 1. Dead pane -> '✕'
    let mut p_dead = PaneInfo::new("%1", "main", "agent", "planner", 1, "claude", true, Some(1));
    p_dead.session = "s-1".to_string();

    // 2. Open query pane -> '?'
    let mut p_query = PaneInfo::new("%2", "main", "agent", "planner", 2, "claude", false, None);
    p_query.session = "s-2".to_string();

    // 3. Active pane -> '●'
    let mut p_active = PaneInfo::new("%3", "main", "agent", "planner", 3, "claude", false, None);
    p_active.session = "s-3".to_string();

    // 4. Stashed pane -> '○'
    let mut p_stash = PaneInfo::new(
        "%4",
        "stash-planner-4",
        "agent",
        "planner",
        4,
        "claude",
        false,
        None,
    );
    p_stash.session = "s-4".to_string();

    let query = Query {
        id: "q-1".to_string(),
        status: "open".to_string(),
        asked_by: "s-2".to_string(),
        task: "t-1".to_string(),
        blocking_scope: "spec".to_string(),
        created: 100.0,
        question: "Help?".to_string(),
        options: vec![],
        answer: String::new(),
        answered_via: String::new(),
    };

    let panes = vec![p_dead, p_query, p_active, p_stash];
    let rows = rows_from(&panes, &[], &[query], Some("%3"), &[]);

    // Header at 0, agents at 1, 2, 3, 4
    if let SidebarRow::Agent { glyph, .. } = &rows[1] {
        assert_eq!(*glyph, '✕');
    } else {
        panic!("expected agent");
    }

    if let SidebarRow::Agent {
        glyph, has_query, ..
    } = &rows[2]
    {
        assert_eq!(*glyph, '?');
        assert!(has_query);
    } else {
        panic!("expected agent");
    }

    if let SidebarRow::Agent {
        glyph, is_active, ..
    } = &rows[3]
    {
        assert_eq!(*glyph, '●');
        assert!(is_active);
    } else {
        panic!("expected agent");
    }

    if let SidebarRow::Agent {
        glyph, is_active, ..
    } = &rows[4]
    {
        assert_eq!(*glyph, '○');
        assert!(!is_active);
    } else {
        panic!("expected agent");
    }
}

#[test]
fn test_sidebar_model_navigation_skips_headers() {
    let p1 = PaneInfo::new("%1", "main", "agent", "planner", 1, "claude", false, None);
    let p2 = PaneInfo::new(
        "%2",
        "stash-implementer-1",
        "agent",
        "implementer",
        1,
        "agy",
        false,
        None,
    );

    let rows = rows_from(&[p1, p2], &[], &[], None, &[]);
    // Rows:
    // 0: Header PLANNERS (not selectable)
    // 1: Agent %1 (selectable)
    // 2: Header IMPLEMENTERS (not selectable)
    // 3: Agent %2 (selectable)
    // 4: AddAgent (selectable)

    assert_eq!(first_selectable(&rows), 1);
    assert_eq!(last_selectable(&rows), 4);

    assert_eq!(next_selectable(&rows, 1), 3);
    assert_eq!(next_selectable(&rows, 3), 4);
    assert_eq!(next_selectable(&rows, 4), 1); // wraps

    assert_eq!(prev_selectable(&rows, 4), 3);
    assert_eq!(prev_selectable(&rows, 3), 1);
    assert_eq!(prev_selectable(&rows, 1), 4); // wraps
}

#[test]
fn test_sidebar_model_selection_grid_flag() {
    let mut p1 = PaneInfo::new("%1", "main", "agent", "planner", 1, "claude", false, None);
    p1.session = "s-1".to_string();
    let mut p2 = PaneInfo::new(
        "%2",
        "stash-planner-2",
        "agent",
        "planner",
        2,
        "claude",
        false,
        None,
    );
    p2.session = "s-2".to_string();

    let selected = vec!["s-1".to_string(), "s-2".to_string()];
    let rows = rows_from(&[p1, p2], &[], &[], None, &selected);

    if let SidebarRow::Agent { is_in_grid, .. } = &rows[1] {
        assert!(is_in_grid);
    } else {
        panic!("expected agent");
    }

    if let SidebarRow::Agent { is_in_grid, .. } = &rows[2] {
        assert!(is_in_grid);
    } else {
        panic!("expected agent");
    }
}
