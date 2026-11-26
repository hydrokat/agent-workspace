mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::adapters::base::{AdapterError, Model};
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::model::{FactorySettings, ModelDecisionAction};
use tandem::factory::model_catalog::{
    CatalogModel, CostBasis, ModelDriftItem, ModelFreshnessStatus, ModelSource,
    verify_configured_models,
};
use tandem::factory::routing::ModelTier;
use tandem::factory::settings::load as load_settings;
use tandem::factory::store::FactoryStore;
use tandem::status::model_picker::{ModelPickerOutcome, ModelUpdatePicker};
use tandem::status::{StatusApp, StatusTab};

struct TempWs {
    root: PathBuf,
    state_dir: PathBuf,
}

impl TempWs {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "tandem-picker-tui-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let root = base.join("ws");
        let state_dir = base.join("state");
        fs::create_dir_all(root.join(".agent-workspace/coordination/queries")).unwrap();
        fs::create_dir_all(&state_dir).unwrap();
        Self { root, state_dir }
    }
}

impl Drop for TempWs {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ch(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}

fn click(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn screen_text(buf: &Buffer, area: Rect) -> String {
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sample_drift() -> Vec<ModelDriftItem> {
    let candidates = vec![
        CatalogModel {
            id: "gemini-3-flash".to_string(),
            label: "Gemini 3 Flash".to_string(),
            tier: ModelTier::Standard,
            cost_rank: 10.0,
            cost_basis: CostBasis::Heuristic,
            allowed: true,
            source: ModelSource::Live,
            unclassified: false,
        },
        CatalogModel {
            id: "gemini-4-neon-high".to_string(),
            label: "Gemini 4 Neon".to_string(),
            tier: ModelTier::Premium,
            cost_rank: 100.0,
            cost_basis: CostBasis::Heuristic,
            allowed: true,
            source: ModelSource::Live,
            unclassified: false,
        },
    ];
    vec![
        ModelDriftItem {
            cli: "agy".to_string(),
            tier: ModelTier::Standard,
            configured: "gemini-2.0-flash".to_string(),
            status: ModelFreshnessStatus::MissingFromCatalog,
            available_models: candidates.clone(),
            available_signature: "sig-v1".to_string(),
            source: ModelSource::Live,
            needs_attention: true,
        },
        ModelDriftItem {
            cli: "agy".to_string(),
            tier: ModelTier::Premium,
            configured: "gemini-2.5-pro".to_string(),
            status: ModelFreshnessStatus::MissingFromCatalog,
            available_models: vec![candidates[1].clone(), candidates[0].clone()],
            available_signature: "sig-v1".to_string(),
            source: ModelSource::Live,
            needs_attention: true,
        },
    ]
}

#[test]
fn model_picker_renders_desktop_and_narrow_and_persists_keyboard_choices() {
    let ws = TempWs::new("kb");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();

    let mut picker = ModelUpdatePicker::new(ws.root.clone(), sample_drift());
    let wide = Rect::new(0, 0, 100, 24);
    let mut wide_buf = Buffer::empty(wide);
    picker.render(wide, &mut wide_buf);
    let wide_txt = screen_text(&wide_buf, wide);
    assert!(wide_txt.contains("Update Model (1/2)"), "{wide_txt}");
    assert!(wide_txt.contains("agy · standard"), "{wide_txt}");
    assert!(wide_txt.contains("gemini-3-flash"), "{wide_txt}");
    assert!(
        wide_txt.contains("Keep current (gemini-2.0-flash)"),
        "{wide_txt}"
    );
    assert!(wide_txt.contains("recommended"), "{wide_txt}");

    let narrow = Rect::new(0, 0, 56, 16);
    let mut narrow_buf = Buffer::empty(narrow);
    picker.render(narrow, &mut narrow_buf);
    let narrow_txt = screen_text(&narrow_buf, narrow);
    assert!(narrow_txt.contains("Update Model (1/2)"), "{narrow_txt}");
    assert!(!narrow_txt.contains("· live)"), "{narrow_txt}");

    // Item 1/2: press Enter to accept recommended first option (`gemini-3-flash`)
    assert_eq!(
        picker.handle_key(&key(KeyCode::Enter)),
        ModelPickerOutcome::Pending
    );
    assert_eq!(picker.current_idx, 1);
    // Item 2/2: press `s` to keep current (`gemini-2.5-pro`)
    let done = picker.handle_key(&ch('s'));
    assert!(matches!(
        done,
        ModelPickerOutcome::Completed { saved: 2, .. }
    ));

    let saved = load_settings(&store).unwrap();
    assert_eq!(saved.model_tiers["agy"].standard, "gemini-3-flash");
    assert_eq!(
        saved.model_decisions["agy"]["standard"].action,
        ModelDecisionAction::Updated
    );
    assert_eq!(saved.model_tiers["agy"].premium, "gemini-2.5-pro");
    assert_eq!(
        saved.model_decisions["agy"]["premium"].action,
        ModelDecisionAction::Kept
    );
}

#[test]
fn model_picker_mouse_select_and_second_click_confirms() {
    let ws = TempWs::new("mouse");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();

    let mut picker = ModelUpdatePicker::new(ws.root.clone(), vec![sample_drift()[0].clone()]);
    let area = Rect::new(0, 0, 100, 24);
    let mut buf = Buffer::empty(area);
    picker.render(area, &mut buf);
    let txt = screen_text(&buf, area);
    let (col, row) = txt
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("gemini-4-neon-high")
                .map(|x| (x as u16 + 1, y as u16))
        })
        .expect("second option rendered");

    // First click moves selection to option 1 without closing
    assert_eq!(picker.mouse_key(&click(col, row)), None);
    assert_eq!(picker.selected_option, 1);

    // Second click confirms & persists chosen model
    let key_ev = picker
        .mouse_key(&click(col, row))
        .expect("second click emits Enter");
    let outcome = picker.handle_key(&key_ev);
    assert!(matches!(
        outcome,
        ModelPickerOutcome::Completed { saved: 1, .. }
    ));
    let saved = load_settings(&store).unwrap();
    assert_eq!(saved.model_tiers["agy"].standard, "gemini-4-neon-high");
}

#[test]
fn status_app_banner_opens_picker_clears_after_update_and_settings_shows_badges() {
    let ws = TempWs::new("app");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();
    let coord = CoordStore::new(ws.root.join(".agent-workspace/coordination"));
    let mut app = StatusApp::new(
        ws.root.clone(),
        coord,
        None,
        Some(Config {
            factory: true,
            state_dir: ws.state_dir.clone(),
            ..Default::default()
        }),
    );

    fn mock_models(cli: &str) -> Result<Vec<Model>, AdapterError> {
        if cli == "agy" {
            Ok(vec![
                Model {
                    id: "gemini-2.0-flash-lite".to_string(),
                    label: "Gemini 2.0 Flash Lite".to_string(),
                },
                Model {
                    id: "gemini-3-flash".to_string(),
                    label: "Gemini 3 Flash".to_string(),
                },
                Model {
                    id: "gemini-4-neon-high".to_string(),
                    label: "Gemini 4 Neon".to_string(),
                },
            ])
        } else {
            Err(AdapterError::ModelList("offline".to_string()))
        }
    }

    let report = verify_configured_models(
        &["agy"],
        &FactorySettings::default(),
        &ws.root.join(".agent-workspace/factory"),
        true,
        &mock_models,
    );
    assert_eq!(report.needs_attention_count(), 2);
    app.set_model_report(report);
    assert!(app.model_banner.is_some());

    let area = Rect::new(0, 0, 110, 24);
    let mut buf = Buffer::empty(area);
    app.render(area, &mut buf);
    let home = screen_text(&buf, area);
    assert!(home.contains("outdated"), "banner missing:\n{home}");

    // Switch to Settings tab and verify [missing]/[outdated] badge renders on Model tiers row
    app.set_tab(StatusTab::Settings);
    app.render(area, &mut buf);
    let settings_txt = screen_text(&buf, area);
    assert!(
        settings_txt.contains("[missing]") || settings_txt.contains("[outdated]"),
        "SettingsTab missing freshness badge:\n{settings_txt}"
    );

    // Press `M` -> opens ModelUpdatePicker modal; accept 1st, keep 2nd -> banner clears!
    app.handle_key(&ch('M'));
    assert!(app.model_picker.is_some());
    app.handle_key(&key(KeyCode::Enter));
    app.handle_key(&ch('s'));
    assert!(app.model_picker.is_none());
    assert!(
        app.model_banner.is_none(),
        "banner must clear once all drift items have been decided"
    );

    // Async runner re-check confirms suppression holds after keeping/updating
    app.trigger_model_check_with(Arc::new(mock_models));
    let settings = load_settings(&store).unwrap();
    let reverify = verify_configured_models(
        &["agy"],
        &settings,
        &ws.root.join(".agent-workspace/factory"),
        true,
        &mock_models,
    );
    assert_eq!(reverify.needs_attention_count(), 0);
}
