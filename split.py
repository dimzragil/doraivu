import sys

with open("src/main.rs", "r") as f:
    lines = f.readlines()

input_start = 163
action_start = 863
suspend_start = 1088
end_match = 1152

input_lines = lines[input_start:action_start]
action_lines = lines[action_start:suspend_start]
suspend_lines = lines[suspend_start:end_match]

# For input, we remove the first line "Event::Input(key) => match app.input_mode {" and the last line "},"
input_inner = "".join(input_lines[1:-1])

# For action, we remove the first line "Event::Action(action) => match action {" and the last line "},"
action_inner = "".join(action_lines[1:-1])

# For suspend, we remove the first line "Event::SuspendAndEdit(file) => {" and the last line "}"
suspend_inner = "".join(suspend_lines[1:-1])

handlers_rs = f"""use crate::app::{{Action, App, Event}};
use crate::{{api, auth, download, trash, ui, upload}};
use crossterm::{{
    event::KeyCode,
    execute,
    terminal::{{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}},
}};
use ratatui::{{backend::CrosstermBackend, Terminal}};
use reqwest::Client;
use tokio::sync::mpsc;
use std::io::Stdout;

pub fn handle_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {{
    match app.input_mode {{
{input_inner}
    }}
}}

pub fn handle_action(
    app: &mut App,
    action: Action,
    client: &Client,
    token: &mut auth::Token,
    auth_info: &auth::AuthInfo,
    tx: &mpsc::Sender<Event>,
) {{
    match action {{
{action_inner}
    }}
}}

pub async fn handle_suspend_and_edit(
    app: &mut App,
    file: crate::app::DriveFile,
    client: &Client,
    token: &auth::Token,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> anyhow::Result<()> {{
{suspend_inner}
    Ok(())
}}
"""

with open("src/handlers.rs", "w") as f:
    f.write(handlers_rs)

# Now modify main.rs
main_rs_new = "".join(lines[:input_start])
main_rs_new += """                Event::Input(key) => {
                    crate::handlers::handle_input(&mut app, key, &client, &token, &tx);
                }
                Event::Action(action) => {
                    crate::handlers::handle_action(&mut app, action, &client, &mut token, &auth_info, &tx);
                }
                Event::SuspendAndEdit(file) => {
                    if let Err(e) = crate::handlers::handle_suspend_and_edit(&mut app, file, &client, &token, &mut terminal).await {
                        app.status = format!("Edit error: {}", e);
                    }
                }
"""
main_rs_new += "".join(lines[end_match:])

with open("src/main.rs", "w") as f:
    f.write(main_rs_new)

