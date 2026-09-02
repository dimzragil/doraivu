import re

with open("src/main.rs", "r") as f:
    lines = f.readlines()

# Extract lines 164 to 862
input_body = "".join(lines[163:863])
action_body = "".join(lines[863:1088])
suspend_body = "".join(lines[1088:1152])

with open("src/handlers.rs", "w") as f:
    f.write("""use crate::app::{Action, App, Event};
use crate::{api, auth, download, trash, ui, upload};
use crossterm::{
    event::KeyCode,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use reqwest::Client;
use tokio::sync::mpsc;
use std::io::Write;

pub async fn handle_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    client: &Client,
    token: &crate::auth::Token,
    tx: &mpsc::Sender<Event>,
) {
""")
    f.write("    match app.input_mode {\n")
    # input_body contains "Event::Input(key) => match app.input_mode {" at the start
    # Let's clean it up.
    
    # Just wrap everything properly.
