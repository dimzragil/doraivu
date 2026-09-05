use anyhow::{Context, Result};
use directories::ProjectDirs;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

// Standard Google Drive scope for full access.
// Can be restricted later if less permission is needed.
const SCOPES: &str = "https://www.googleapis.com/auth/drive";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub token_type: String,
    pub scope: String,
}

#[derive(Deserialize, Debug)]
struct RefreshResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AuthInfo {
    pub client_id: String,
    pub client_secret: String,
}

pub fn get_config_dir() -> Result<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("", "", "doraivu") {
        let config_dir = proj_dirs.config_dir();
        fs::create_dir_all(config_dir).context("Failed to create config directory")?;
        Ok(config_dir.to_path_buf())
    } else {
        anyhow::bail!("Could not determine configuration directory");
    }
}

pub fn get_token_path() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("token.json"))
}

pub fn get_credentials_path() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("credentials.json"))
}

pub fn load_credentials() -> Result<Option<AuthInfo>> {
    let path = get_credentials_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    let auth = serde_json::from_str(&content)?;
    Ok(Some(auth))
}

pub fn save_credentials(auth: &AuthInfo) -> Result<()> {
    let path = get_credentials_path()?;
    let content = serde_json::to_string_pretty(auth)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn load_token() -> Result<Option<Token>> {
    let path = get_token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    let token = serde_json::from_str(&content)?;
    Ok(Some(token))
}

pub fn save_token(token: &Token) -> Result<()> {
    let path = get_token_path()?;
    let content = serde_json::to_string_pretty(token)?;
    fs::write(&path, content)?;
    Ok(())
}

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

pub async fn authenticate(client: &Client, auth_info: &AuthInfo) -> Result<Token> {
    if let Some(token) = load_token()? {
        println!("Loaded existing token from config.");
        return Ok(token);
    }

    println!("No token found. Starting Localhost Webserver Flow...");

    // Bind to any available port on localhost
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    let redirect_uri = format!("http://127.0.0.1:{}", local_addr.port());

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline",
        GOOGLE_AUTH_URL,
        urlencoding::encode(&auth_info.client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(SCOPES)
    );

    println!("===========================================================");
    println!("Please check your browser. If it did not open automatically,");
    println!("manually navigate to this URL:");
    println!("{}", auth_url);
    println!("===========================================================");
    println!("Waiting for authorization (listening on {})...", local_addr);

    // Attempt to open the browser automatically
    if let Err(e) = open::that(&auth_url) {
        println!("Failed to open browser automatically: {}", e);
    }

    let mut auth_code = None;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = [0; 4096];
        let read_result =
            tokio::time::timeout(std::time::Duration::from_millis(500), stream.read(&mut buf))
                .await;
        let n = match read_result {
            Ok(Ok(size)) => size,
            _ => continue,
        };
        if n == 0 {
            continue;
        }

        let request = String::from_utf8_lossy(&buf[..n]);
        let mut found_code = false;

        for line in request.lines() {
            if line.starts_with("GET ") {
                if let Some(path) = line.split_whitespace().nth(1) {
                    if let Some(query) = path.split('?').nth(1) {
                        for pair in query.split('&') {
                            let mut parts = pair.split('=');
                            if parts.next() == Some("code") {
                                if let Some(code) = parts.next() {
                                    auth_code = Some(code.to_string());
                                    found_code = true;
                                }
                            }
                        }
                    }
                }
                break;
            }
        }

        if found_code {
            let body =
                "Authorization successful! You can close this tab and return to the terminal.";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
            let _ = stream.flush().await;
            break;
        } else {
            let body = "Not found";
            let resp = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
            let _ = stream.flush().await;
        }
    }

    let Some(code) = auth_code else {
        anyhow::bail!("No authorization code found in HTTP request.");
    };

    // Exchange code for token
    let res = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", auth_info.client_id.as_str()),
            ("client_secret", auth_info.client_secret.as_str()),
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await?;
        anyhow::bail!("Token exchange failed. Status: {}. Body: {}", status, text);
    }

    let token: Token = res.json().await?;
    println!("Successfully authenticated!");
    save_token(&token)?;
    println!("Token saved to {:?}", get_token_path()?);

    Ok(token)
}

pub async fn refresh_token_if_needed(
    client: &Client,
    auth_info: &AuthInfo,
    token: &mut Token,
) -> Result<()> {
    let Some(refresh_token) = &token.refresh_token else {
        anyhow::bail!("No refresh token available");
    };

    let res = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", auth_info.client_id.as_str()),
            ("client_secret", auth_info.client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?;

    if res.status().is_success() {
        let refresh_res: RefreshResponse = res.json().await?;
        token.access_token = refresh_res.access_token;
        token.expires_in = refresh_res.expires_in;
        // Keep the old refresh_token

        save_token(token)?;
        Ok(())
    } else {
        let body = res.text().await?;
        anyhow::bail!("Failed to refresh token: {}", body);
    }
}
