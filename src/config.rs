use clap::Parser;

#[derive(Clone, Debug, Parser)]
pub struct Config {
    #[arg(short, long, default_value_t = 8080)]
    pub port: i16,
    #[arg(long)]
    pub proxy: String,
}
