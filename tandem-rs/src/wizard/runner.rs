use std::collections::HashMap;
use std::sync::Arc;

use crate::adapters::base::{AgentSpec, Model};
use crate::wizard::app::{ModelLoader, WizardApp};
use crate::wizard::flow::CLIS;

pub fn installed_clis() -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for cli in CLIS {
        map.insert(
            cli.to_string(),
            crate::versions::default_which(cli).is_some(),
        );
    }
    map
}

pub fn load_models(cli: &str) -> Result<Vec<Model>, String> {
    let adapter = crate::adapters::adapter_for(cli).map_err(|e| e.to_string())?;
    adapter.list_models().map_err(|e| e.to_string())
}

pub fn run_launch_wizard(
    installed: Option<HashMap<String, bool>>,
    loader: Option<ModelLoader>,
) -> Result<Option<Vec<AgentSpec>>, std::io::Error> {
    let installed = installed.unwrap_or_else(installed_clis);
    let loader = loader.unwrap_or_else(|| Arc::new(load_models));
    let mut app = WizardApp::new(installed, loader, false);
    run_app_in_terminal(&mut app)
}

pub fn run_add_agent(
    installed: Option<HashMap<String, bool>>,
    loader: Option<ModelLoader>,
) -> Result<Option<AgentSpec>, std::io::Error> {
    let installed = installed.unwrap_or_else(installed_clis);
    let loader = loader.unwrap_or_else(|| Arc::new(load_models));
    let mut app = WizardApp::new(installed, loader, true);
    let result = run_app_in_terminal(&mut app)?;
    Ok(result.and_then(|mut agents| agents.pop()))
}

fn run_app_in_terminal(app: &mut WizardApp) -> Result<Option<Vec<AgentSpec>>, std::io::Error> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let res = app.run(&mut terminal);

    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    res
}
