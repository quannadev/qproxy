use clap::Parser;
use tokio::time;
use qproxy::{Config, ForwardProxy};


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse();
    let server = ForwardProxy::new(config.port as u16, config.proxy)?;
    let server_clone = server.clone();
    tokio::spawn(async move {
        time::sleep(time::Duration::from_secs(4)).await;
        server_clone.stop();
    });
    server.start().map_err(|e| e.into())
}
