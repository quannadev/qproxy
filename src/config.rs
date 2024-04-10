use clap::Parser;

#[derive(Clone, Debug, Parser)]
pub struct Config {
    #[arg(short, long, default_value_t = 8080)]
    pub port: i16,
    #[arg(long, default_value = "proxies.txt")]
    pub proxies_path: String,
    #[arg(long, default_value_t = 300)] //in seconds 5m = 60 * 5 = 300
    pub rotate_interval: i64, //in seconds 10p = 60 * 10 = 600
}
