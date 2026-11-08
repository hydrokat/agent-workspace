use std::collections::HashMap;
use std::sync::Arc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use tandem::adapters::base::Model;
use tandem::wizard::app::{ModelLoader, WizardApp};

use crate::common::assert_golden;

fn buffer_text(buf: &Buffer) -> String {
    let mut s = String::new();
    let area = buf.area;
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell((x, y)) {
                s.push_str(cell.symbol());
            }
        }
        s.push('\n');
    }
    s
}

fn test_loader() -> ModelLoader {
    Arc::new(|cli: &str| {
        if cli == "agy" {
            Ok(vec![
                Model {
                    id: "gemini-3.8-flash-high".to_string(),
                    label: "Flash High".to_string(),
                },
                Model {
                    id: "gemini-3.8-flash-low".to_string(),
                    label: "Flash Low".to_string(),
                },
            ])
        } else {
            Ok(vec![
                Model {
                    id: "opus".to_string(),
                    label: "opus (latest)".to_string(),
                },
                Model {
                    id: "sonnet".to_string(),
                    label: "sonnet (latest)".to_string(),
                },
            ])
        }
    })
}

#[test]
fn test_golden_wizard_screens() {
    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    installed.insert("agy".to_string(), true);

    // 1. Initial screen
    let mut app = WizardApp::new(installed.clone(), test_loader(), false);
    let backend = TestBackend::new(100, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    let initial_text = buffer_text(term.backend().buffer());
    assert_golden("wizard/initial_screen.txt", &initial_text);

    // 2. Help popup
    app.show_help = true;
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    let help_text = buffer_text(term.backend().buffer());
    assert_golden("wizard/help_screen.txt", &help_text);

    // 3. Single agent initial screen
    let mut single_app = WizardApp::new(installed, test_loader(), true);
    let mut single_term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    single_term
        .draw(|f| single_app.render(f.area(), f.buffer_mut()))
        .unwrap();
    let single_text = buffer_text(single_term.backend().buffer());
    assert_golden("wizard/single_agent_screen.txt", &single_text);
}
