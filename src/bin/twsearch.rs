use cancel_culture::browser::make_client_or_panic;
use clap::{crate_authors, crate_version, Clap};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stderr,
    );

    let opts: Opts = Opts::parse();

    let mut browser = make_client_or_panic(
        &opts.browser,
        !opts.disable_headless,
        opts.host.as_deref(),
        opts.port,
    )
    .await;

    let client = cancel_culture::twitter::Client::from_config_file(&opts.key_file).await?;
    let mut lister = cancel_culture::twitter::TweetLister::new(&client, &mut browser);

    let (mut ids, expected) = lister.get_all(opts.screen_name).await?;

    log::info!("Found: {}, expected: {}", ids.len(), expected);

    ids.sort_unstable();

    for id in ids {
        println!("{}", id);
    }

    Ok(())
}

#[derive(Clap)]
#[clap(version = crate_version!(), author = crate_authors!())]
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
    browser: String,
}
