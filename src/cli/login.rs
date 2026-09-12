use anyhow::Result;

pub async fn run() -> Result<()> {
    crate::cli::setup::run_with_title("Doraivu - Login").await
}
