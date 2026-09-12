pub mod login;
pub mod logout;
pub mod setup;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "doraivu",
    about = "Pure Rust TUI & CLI client for Google Drive",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Google OAuth Client ID
    #[arg(long, env = "GOOGLE_CLIENT_ID")]
    pub client_id: Option<String>,

    /// Google OAuth Client Secret
    #[arg(long, env = "GOOGLE_CLIENT_SECRET")]
    pub client_secret: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Configure Google OAuth credentials and authenticate
    Setup,
    /// Log in to Google Drive account
    Login,
    /// Log out from Google Drive account
    Logout,
}
