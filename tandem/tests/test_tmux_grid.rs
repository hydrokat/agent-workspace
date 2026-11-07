use tandem::tmux::grid::{FillOrder, GridCols, GridPrefs, GridRows, Rect, clamp_offset, plan_grid};

#[test]
fn test_plan_grid_empty_selection() {
    let prefs = GridPrefs::default();
    let plan = plan_grid(0, 160, 40, &prefs, 0);
    assert_eq!(plan.cols, 0);
    assert_eq!(plan.total_rows, 0);
    assert_eq!(plan.visible_range, 0..0);
    assert!(plan.cells.is_empty());
    assert!(!plan.has_overflow);
}

#[test]
fn test_plan_grid_single_agent() {
    let prefs = GridPrefs::default();
    let plan = plan_grid(1, 160, 40, &prefs, 0);
    assert_eq!(plan.cols, 1);
    assert_eq!(plan.total_rows, 1);
    assert_eq!(plan.rows_per_page, 3); // 40 / 12 = 3
    assert_eq!(plan.visible_range, 0..1);
    assert_eq!(plan.cells.len(), 1);
    assert_eq!(
        plan.cells[0],
        Rect {
            x: 0,
            y: 0,
            width: 160,
            height: 40,
        }
    );
    assert!(!plan.has_overflow);
}

#[test]
fn test_plan_grid_auto_columns_by_viewport_width() {
    let prefs = GridPrefs::default();

    // 100 cols -> 100 / 60 = 1 col
    let p1 = plan_grid(4, 100, 40, &prefs, 0);
    assert_eq!(p1.cols, 1);

    // 130 cols -> 130 / 60 = 2 cols
    let p2 = plan_grid(4, 130, 40, &prefs, 0);
    assert_eq!(p2.cols, 2);

    // 200 cols -> 200 / 60 = 3 cols
    let p3 = plan_grid(6, 200, 40, &prefs, 0);
    assert_eq!(p3.cols, 3);

    // 300 cols -> capped at MAX_COLS (4)
    let p4 = plan_grid(8, 300, 40, &prefs, 0);
    assert_eq!(p4.cols, 4);
}

#[test]
fn test_plan_grid_last_row_stretches_to_fill_width() {
    let prefs = GridPrefs {
        cols: GridCols::Fixed(2),
        rows: GridRows::Fixed(2),
        fill: FillOrder::RowFirst,
    };
    // 3 agents in 2x2 grid:
    // Row 0 has 2 agents (x=0 w=80, x=80 w=80)
    // Row 1 has 1 agent (x=0 w=160 full width)
    let plan = plan_grid(3, 160, 40, &prefs, 0);
    assert_eq!(plan.cols, 2);
    assert_eq!(plan.total_rows, 2);
    assert_eq!(plan.cells.len(), 3);

    // First row
    assert_eq!(plan.cells[0].x, 0);
    assert_eq!(plan.cells[0].width, 80);
    assert_eq!(plan.cells[1].x, 80);
    assert_eq!(plan.cells[1].width, 80);

    // Second row (last row with 1 agent stretches)
    assert_eq!(plan.cells[2].x, 0);
    assert_eq!(plan.cells[2].width, 160);
    assert_eq!(plan.cells[2].y, 20);
    assert_eq!(plan.cells[2].height, 20);
}

#[test]
fn test_plan_grid_overflow_allocates_gridbar_space() {
    let prefs = GridPrefs {
        cols: GridCols::Fixed(2),
        rows: GridRows::Fixed(2),
        fill: FillOrder::RowFirst,
    };
    // 7 agents with 2 cols = 4 total rows. Viewport has 2 rows -> overflow!
    let plan = plan_grid(7, 161, 40, &prefs, 0);
    assert_eq!(plan.total_rows, 4);
    assert_eq!(plan.rows_per_page, 2);
    assert!(plan.has_overflow);

    // Viewport width is 161, but 1 col is reserved for gridbar -> cells use 160 cols
    assert_eq!(plan.cells[0].width + plan.cells[1].width, 160);
    assert_eq!(plan.visible_range, 0..4);
}

#[test]
fn test_clamp_offset() {
    assert_eq!(clamp_offset(0, 4, 2), 0);
    assert_eq!(clamp_offset(1, 4, 2), 1);
    assert_eq!(clamp_offset(2, 4, 2), 2);
    assert_eq!(clamp_offset(3, 4, 2), 2); // clamped to total_rows - rows_per_page
    assert_eq!(clamp_offset(10, 4, 2), 2);
    assert_eq!(clamp_offset(5, 2, 3), 0); // no overflow
}

#[test]
fn test_plan_grid_offsets_and_paging() {
    let prefs = GridPrefs {
        cols: GridCols::Fixed(2),
        rows: GridRows::Fixed(2),
        fill: FillOrder::RowFirst,
    };
    // 7 agents, 4 rows total
    // offset 0: agents 0..4 (rows 0 and 1)
    let p0 = plan_grid(7, 161, 40, &prefs, 0);
    assert_eq!(p0.visible_range, 0..4);

    // offset 1: agents 2..6 (rows 1 and 2)
    let p1 = plan_grid(7, 161, 40, &prefs, 1);
    assert_eq!(p1.visible_range, 2..6);

    // offset 2: agents 4..7 (rows 2 and 3; row 3 has 1 agent)
    let p2 = plan_grid(7, 161, 40, &prefs, 2);
    assert_eq!(p2.visible_range, 4..7);
    assert_eq!(p2.cells.len(), 3);
}

#[test]
fn test_plan_grid_table_all_counts_and_viewports() {
    let viewports = [(240, 60), (130, 36), (65, 24)];
    let col_prefs = [
        GridCols::Auto,
        GridCols::Fixed(1),
        GridCols::Fixed(2),
        GridCols::Fixed(3),
        GridCols::Fixed(4),
    ];
    let row_prefs = [
        GridRows::Auto,
        GridRows::Fixed(1),
        GridRows::Fixed(2),
        GridRows::Fixed(3),
    ];
    let fill_orders = [FillOrder::RowFirst, FillOrder::ColumnFirst];

    for (vw, vh) in viewports {
        for cp in &col_prefs {
            for rp in &row_prefs {
                for fo in &fill_orders {
                    let prefs = GridPrefs {
                        cols: *cp,
                        rows: *rp,
                        fill: *fo,
                    };

                    for n in 1..=12 {
                        for offset in 0..=5 {
                            let plan = plan_grid(n, vw, vh, &prefs, offset);

                            assert_eq!(plan.visible_range.len(), plan.cells.len());
                            assert!(plan.cols >= 1 && plan.cols <= 4);
                            assert!(plan.rows_per_page >= 1);
                            assert!(plan.total_rows >= 1);

                            let max_w = if plan.has_overflow { vw - 1 } else { vw };
                            for cell in &plan.cells {
                                assert!(cell.x + cell.width <= max_w);
                                assert!(cell.y + cell.height <= vh);
                                assert!(cell.width > 0);
                                assert!(cell.height > 0);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn test_grid_operations_property_invariants() {
    struct Rng(u64);
    impl Rng {
        fn next_u32(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            (self.0 >> 32) as u32
        }
        fn gen_range(&mut self, low: usize, high: usize) -> usize {
            if low >= high {
                return low;
            }
            low + (self.next_u32() as usize % (high - low))
        }
    }

    let mut rng = Rng(0xCAFE_BABE_DEAD_BEEF);

    for _ in 0..1000 {
        let n_agents = rng.gen_range(1, 15);
        let mut selected = vec![0];
        let mut offset = 0;

        let vw = rng.gen_range(50, 260);
        let vh = rng.gen_range(15, 80);
        let prefs = GridPrefs::default();

        let n_ops = rng.gen_range(10, 30);
        for _ in 0..n_ops {
            let op = rng.gen_range(0, 5);
            match op {
                0 => {
                    let agent = rng.gen_range(0, n_agents);
                    if let Some(pos) = selected.iter().position(|&x| x == agent) {
                        if selected.len() > 1 {
                            selected.remove(pos);
                        }
                    } else {
                        selected.push(agent);
                    }
                }
                1 => {
                    let agent = rng.gen_range(0, n_agents);
                    selected = vec![agent];
                }
                2 => {
                    let delta = rng.gen_range(0, 3);
                    offset += delta;
                }
                3 => {
                    let delta = rng.gen_range(0, 3);
                    offset = offset.saturating_sub(delta);
                }
                4 => {
                    let agent = rng.gen_range(0, n_agents);
                    selected.retain(|&x| x != agent);
                    if selected.is_empty() {
                        selected.push(if agent == 0 { 1.min(n_agents - 1) } else { 0 });
                    }
                }
                _ => {}
            }

            assert!(!selected.is_empty());

            let plan = plan_grid(selected.len(), vw, vh, &prefs, offset);
            assert_eq!(plan.visible_range.len(), plan.cells.len());
            assert!(plan.visible_range.end <= selected.len());

            let max_w = if plan.has_overflow { vw - 1 } else { vw };
            for cell in &plan.cells {
                assert!(cell.x + cell.width <= max_w);
                assert!(cell.y + cell.height <= vh);
            }
        }
    }
}
