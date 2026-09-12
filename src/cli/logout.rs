use anyhow::Result;
use crossterm::style::Stylize;
use reqwest::Client;
use std::io::{self, Write};

use crate::drive::auth;

pub async fn run() -> Result<()> {
    println!();
    println!("{}", "╭──────────────────────────────────────────────────────────╮".cyan());
    println!("{}", "│                 Doraivu - Logout                         │".cyan().bold());
    println!("{}", "╰──────────────────────────────────────────────────────────╯".cyan());

    let token_exists = auth::load_token()?.is_some();
    let creds_exists = auth::load_credentials()?.is_some();

    if !token_exists && !creds_exists {
        println!("{}", "No active session or credentials found. Already logged out.".yellow());
        return Ok(());
    }

    if token_exists {
        print!("Are you sure you want to log out? [y/N]: ");
        io::stdout().flush()?;
        let mut ans = String::new();
        if io::stdin().read_line(&mut ans)? == 0 || !ans.trim().eq_ignore_ascii_case("y") {
            println!("Logout cancelled.");
            return Ok(());
        }

        print!("\nRevoking token and clearing session... ");
        io::stdout().flush()?;

        let client = Client::builder()
            .user_agent("doraivu-rust-client/1.0")
            .build()?;

        let _ = auth::revoke_token(&client).await;
        let _ = auth::clear_token();
        println!("{}", "✓ Done".green().bold());
        println!("{}", "✓ Successfully logged out from your Google Drive account.".green());
    } else {
        println!("{}", "No active login token found.".yellow());
    }

    if creds_exists {
        print!("\nDo you also want to remove saved OAuth credentials (client_id & secret)? [y/N]: ");
        io::stdout().flush()?;
        let mut ans = String::new();
        if io::stdin().read_line(&mut ans)? > 0 && ans.trim().eq_ignore_ascii_case("y") {
            let _ = auth::clear_credentials();
            println!("{}", "✓ Credentials removed.".green());
        } else {
            println!("OAuth credentials kept. You can run '{}' anytime to log in again.", "doraivu login".cyan());
        }
    }

    println!();
    Ok(())
}
