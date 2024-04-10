use clap::Parser;
use qproxy::{Config, ProxyManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::try_init().unwrap_or_default();
    let config = Config::parse();
    // let server = ProxyServer::try_from((config.port, config.proxy))?;
    // let cloned_server = server.clone();
    // let handle = tokio::spawn(async move { server.start() });
    // time::sleep(time::Duration::from_secs(10)).await;
    // cloned_server.stop();
    // handle.abort();

    let manager =
        ProxyManager::new(&config.proxies_path, config.port, config.rotate_interval).await;
    manager.start().await?;

    Ok(())
}
