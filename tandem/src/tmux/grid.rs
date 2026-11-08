use std::ops::Range;

pub const MIN_CELL_COLS: usize = 60;
pub const MIN_CELL_ROWS: usize = 12;
pub const MAX_COLS: usize = 4;
pub const MAX_ROWS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SidebarMode {
    #[default]
    Auto,
    Wide,
    Compact,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Breakpoint {
    #[default]
    Wide,
    Medium,
    Narrow,
}

impl Breakpoint {
    pub fn default_sidebar_cols(&self) -> Option<usize> {
        match self {
            Breakpoint::Wide => Some(26),
            Breakpoint::Medium => Some(14),
            Breakpoint::Narrow => None,
        }
    }

    pub fn max_grid_cols(&self) -> usize {
        match self {
            Breakpoint::Wide => 4,
            Breakpoint::Medium => 2,
            Breakpoint::Narrow => 1,
        }
    }

    pub fn popup_percent(&self) -> (usize, usize) {
        match self {
            Breakpoint::Wide => (70, 70),
            Breakpoint::Medium => (90, 90),
            Breakpoint::Narrow => (100, 100),
        }
    }
}

pub fn breakpoint(cols: usize, rows: usize) -> Breakpoint {
    if cols >= 120 && rows >= 30 {
        Breakpoint::Wide
    } else if cols >= 80 && rows >= 20 {
        Breakpoint::Medium
    } else {
        Breakpoint::Narrow
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GridCols {
    #[default]
    Auto,
    Fixed(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GridRows {
    #[default]
    Auto,
    Fixed(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FillOrder {
    #[default]
    RowFirst,
    ColumnFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GridPrefs {
    pub cols: GridCols,
    pub rows: GridRows,
    pub fill: FillOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridPlan {
    pub cols: usize,
    pub rows_per_page: usize,
    pub total_rows: usize,
    pub visible_range: Range<usize>,
    pub cells: Vec<Rect>,
    pub has_overflow: bool,
}

pub fn clamp_offset(offset: usize, total_rows: usize, rows_per_page: usize) -> usize {
    offset.min(total_rows.saturating_sub(rows_per_page))
}

pub fn plan_grid(
    n_selected: usize,
    viewport_cols: usize,
    viewport_rows: usize,
    prefs: &GridPrefs,
    offset: usize,
) -> GridPlan {
    if n_selected == 0 {
        return GridPlan {
            cols: 0,
            rows_per_page: 0,
            total_rows: 0,
            visible_range: 0..0,
            cells: Vec::new(),
            has_overflow: false,
        };
    }

    let tentative_cols = match prefs.cols {
        GridCols::Fixed(c) => c.clamp(1, MAX_COLS).min(n_selected),
        GridCols::Auto => (viewport_cols / MIN_CELL_COLS)
            .clamp(1, MAX_COLS)
            .min(n_selected),
    };

    let rows_per_page = match prefs.rows {
        GridRows::Fixed(r) => r.clamp(1, MAX_ROWS),
        GridRows::Auto => (viewport_rows / MIN_CELL_ROWS).clamp(1, MAX_ROWS),
    };

    let total_rows_tentative = n_selected.div_ceil(tentative_cols);
    let has_overflow = total_rows_tentative > rows_per_page;

    let usable_cols = if has_overflow {
        viewport_cols.saturating_sub(1)
    } else {
        viewport_cols
    };

    let cols = match prefs.cols {
        GridCols::Fixed(c) => c.clamp(1, MAX_COLS).min(n_selected),
        GridCols::Auto => (usable_cols / MIN_CELL_COLS)
            .clamp(1, MAX_COLS)
            .min(n_selected),
    };

    let total_rows = n_selected.div_ceil(cols);
    let has_overflow = total_rows > rows_per_page;
    let safe_offset = clamp_offset(offset, total_rows, rows_per_page);

    let visible_rows = (total_rows - safe_offset).min(rows_per_page);
    let start_idx = safe_offset * cols;
    let end_idx = (start_idx + visible_rows * cols).min(n_selected);
    let visible_range = start_idx..end_idx;

    let mut cells = Vec::new();
    if let Some(base_row_h) = viewport_rows.checked_div(visible_rows) {
        let rem_row_h = viewport_rows % visible_rows;

        let mut curr_y = 0;
        for r in 0..visible_rows {
            let row_h = base_row_h + if r < rem_row_h { 1 } else { 0 };
            let abs_row = safe_offset + r;
            let agents_in_row = if abs_row == total_rows - 1 {
                let rem = n_selected % cols;
                if rem == 0 { cols } else { rem }
            } else {
                cols
            };

            let base_col_w = usable_cols / agents_in_row;
            let rem_col_w = usable_cols % agents_in_row;

            let mut curr_x = 0;
            for c in 0..agents_in_row {
                let col_w = base_col_w + if c < rem_col_w { 1 } else { 0 };
                cells.push(Rect {
                    x: curr_x,
                    y: curr_y,
                    width: col_w,
                    height: row_h,
                });
                curr_x += col_w;
            }
            curr_y += row_h;
        }
    }

    GridPlan {
        cols,
        rows_per_page,
        total_rows,
        visible_range,
        cells,
        has_overflow,
    }
}
