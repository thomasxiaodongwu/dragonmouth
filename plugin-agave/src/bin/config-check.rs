use {clap::Parser, richat_plugin_agave::config::Config};

#[derive(Debug, Parser)]
#[clap(
    author,
    version,
    about = "Richat Agave Geyser Plugin Config Check Cli Tool"
)]
struct Args {
    #[clap(short, long, default_value_t = String::from("config.json"))]
    /// Path to config
    config: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let _config = Config::load_from_file(args.config)?;
    println!("Config is OK!");
    Ok(())
}
