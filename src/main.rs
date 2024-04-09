use clap::Parser;
use qproxy::{Config, ForwardProxy};


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse();
    let server = ForwardProxy::new(config.port as u16, config.proxy)?;
    server.start().map_err(|e| e.into())
}
