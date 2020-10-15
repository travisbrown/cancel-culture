use cancelculture::browser;
use cancelculture::browser::twitter::search::UserTweetSearch;
use cancelculture::browser::Scroller;
use clap::{crate_authors, crate_version, Clap};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stderr,
    );

    let opts: Opts = Opts::parse();

    let mut client = browser::make_client_or_panic(
        &opts.browser,
        !opts.disable_headless,
        opts.host.as_deref(),
        opts.port,
    )
    .await;

    let search = UserTweetSearch::parse(&opts.screen_name, &opts.from, &opts.to)?;
    let urls = search.extract_all(&mut client).await?;

    log::info!("Found {}", urls.len());

    for url in urls {
        println!("{}", url);
    }

    Ok(())
}

#[derive(Clap)]
#[clap(version = crate_version!(), author = crate_authors!())]
struct Opts {
    #[clap(short = 'u', long)]
    screen_name: String,
    #[clap(short = 'f', long)]
    from: String,
    #[clap(short = 't', long)]
    to: String,
    #[clap(short, long)]
    host: Option<String>,
    #[clap(short, long)]
    port: Option<u16>,
    #[clap(short = 'n', long)]
    disable_headless: bool,
    #[clap(short, long, default_value = "chrome")]
    browser: String,
}
