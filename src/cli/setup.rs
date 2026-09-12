use anyhow::Result;
use crossterm::style::Stylize;
use reqwest::Client;
use std::io::{self, Write};

use crate::drive::auth::{self, AuthInfo};

pub async fn run() -> Result<()> {
    run_with_title("Doraivu - Initial Setup").await
}

pub async fn run_with_title(title: &str) -> Result<()> {
    println!();
    println!("{}", "╭──────────────────────────────────────────────────────────╮".cyan());
    let title_line = format!("│{:^58}│", title);
    println!("{}", title_line.cyan().bold());
    println!("{}", "╰──────────────────────────────────────────────────────────╯".cyan());
    println!("Configure your Google OAuth 2.0 Client credentials.\n");
    println!("Need help creating credentials?");
    println!("  1. Open Google Cloud Console: {}", "https://console.cloud.google.com/".underlined());
    println!("  2. Create a project and enable 'Google Drive API'.");
    println!("  3. Go to 'APIs & Services' > 'Credentials'.");
    println!("  4. Click 'Create Credentials' > 'OAuth client ID' (Type: 'Desktop app').");
    println!("  5. Copy your Client ID and Client Secret below.");
    println!("{}", "────────────────────────────────────────────────────────────".dark_grey());
    println!();

    if let Ok(Some(existing)) = auth::load_credentials() {
        println!("Existing credentials found:");
        println!("  Client ID: {}", auth::mask_client_id(&existing.client_id));
        print!("\nDo you want to reconfigure credentials? [y/N]: ");
        io::stdout().flush()?;
        let mut ans = String::new();
        if io::stdin().read_line(&mut ans)? == 0 || !ans.trim().eq_ignore_ascii_case("y") {
            println!("\nExisting credentials kept.");
            return prompt_login_flow(&existing).await;
        }
        println!();
    }

    let client = Client::builder()
        .user_agent("doraivu-rust-client/1.0")
        .build()?;

    let auth_info = prompt_credentials_with_validation(&client).await?;

    auth::save_credentials(&auth_info)?;
    let _ = auth::clear_token();
    println!("\n{}", "✓ Credentials saved successfully!".green().bold());

    prompt_login_flow(&auth_info).await
}

async fn prompt_credentials_with_validation(client: &Client) -> Result<AuthInfo> {
    loop {
        let client_id = prompt_input("Enter Google Client ID: ")?;
        let client_secret = prompt_input("Enter Google Client Secret: ")?;

        let info = AuthInfo {
            client_id,
            client_secret,
        };

        print!("\nValidating credentials with Google OAuth... ");
        io::stdout().flush()?;

        match auth::validate_credentials(client, &info).await {
            Ok(()) => {
                println!("{}", "✓ Valid!".green().bold());
                return Ok(info);
            }
            Err(e) => {
                println!("{}", "✗ Error".red().bold());
                println!("  {}", format!("{:#}", e).red());
                print!("\nDo you want to try entering credentials again? [Y/n]: ");
                io::stdout().flush()?;
                let mut ans = String::new();
                if io::stdin().read_line(&mut ans)? == 0 || ans.trim().eq_ignore_ascii_case("n") {
                    anyhow::bail!("Setup aborted by user.");
                }
                println!();
            }
        }
    }
}

fn prompt_input(prompt: &str) -> Result<String> {
    loop {
        print!("{}", prompt);
        io::stdout().flush()?;
        let mut input = String::new();
        let bytes = io::stdin().read_line(&mut input)?;
        if bytes == 0 {
            anyhow::bail!("Input stream closed (EOF).");
        }
        let trimmed = input.trim();
        if trimmed.is_empty() {
            println!("{}", "  Error: Input cannot be empty. Please try again.".yellow());
            continue;
        }
        return Ok(trimmed.to_string());
    }
}

async fn prompt_login_flow(auth_info: &AuthInfo) -> Result<()> {
    if let Ok(Some(_)) = auth::load_token() {
        println!("Existing token found. You are already logged in.");
        print!("Do you want to re-authenticate via browser? [y/N]: ");
        io::stdout().flush()?;
        let mut ans = String::new();
        if io::stdin().read_line(&mut ans)? == 0 || !ans.trim().eq_ignore_ascii_case("y") {
            println!("\nSetup complete! You can run '{}' to start Doraivu.", "doraivu".cyan().bold());
            return Ok(());
        }
        let _ = auth::clear_token();
    }

    println!("{}", "────────────────────────────────────────────────────────────".dark_grey());
    print!("Proceed to browser login now? [Y/n]: ");
    io::stdout().flush()?;
    let mut ans = String::new();
    let bytes = io::stdin().read_line(&mut ans)?;
    let ans_trimmed = ans.trim();
    if bytes == 0 || ans_trimmed.eq_ignore_ascii_case("n") {
        println!("\nSetup saved. You can authenticate later by running '{}'.", "doraivu".cyan());
        return Ok(());
    }

    println!();
    let client = Client::builder()
        .user_agent("doraivu-rust-client/1.0")
        .build()?;

    match auth::authenticate(&client, auth_info).await {
        Ok(_) => {
            println!();
            println!("{}", "===========================================================".green());
            println!("{}", "  ✓ Authentication successful! Setup is complete.".green().bold());
            println!("  You can now launch Doraivu by running: {}", "doraivu".cyan().bold());
            println!("{}", "===========================================================".green());
            Ok(())
        }
        Err(e) => {
            println!();
            println!("{}", format!("✗ Authentication failed: {:#}", e).red().bold());
            println!("You can retry setup anytime with '{}'.", "doraivu setup".cyan());
            Err(e)
        }
    }
}
