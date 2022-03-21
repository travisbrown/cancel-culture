use cancel_culture::{browser::make_client_or_panic, cli};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose)?;

    let mut browser = make_client_or_panic(
        &opts.browser,
        !opts.disable_headless,
        opts.host.as_deref(),
        opts.port,
    )
    .await;

    let client = egg_mode_extras::Client::from_config_file(&opts.key_file).await?;
    let mut lister = cancel_culture::browser::twitter::TweetLister::new(&client, &mut browser);

    let (mut ids, expected) = lister.get_all(opts.screen_name).await?;

    log::info!("Found: {}, expected: {}", ids.len(), expected);

    ids.sort_unstable();

    for id in ids {
        println!("{}", id);
    }

    log::logger().flush();

    Ok(())
}

#[derive(Parser)]
#[clap(version, author)]
struct Opts {
    /// TOML file containing Twitter API keys
    #[clap(short, long, default_value = "keys.toml")]
    key_file: String,
    #[clap(short = 'u', long)]
    screen_name: String,
    #[clap(short, long)]
    host: Option<String>,
    #[clap(short, long)]
    port: Option<u16>,
    #[clap(short = 'n', long)]
    disable_headless: bool,
    #[clap(short, long, default_value = "chrome")]
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    browser: String,
}
