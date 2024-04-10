use clap::Parser;
use qproxy::{Config, ProxyManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::try_init().unwrap_or_default();
    let config = Config::parse();
    let manager = ProxyManager::new(&config).await;
    manager.start().await?;

    Ok(())
}
