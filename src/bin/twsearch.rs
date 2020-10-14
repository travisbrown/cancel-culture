use cancelculture::browser;
use chrono::NaiveDate;
use clap::{crate_authors, crate_version, Clap};
use futures::TryStreamExt;

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

    let from = NaiveDate::parse_from_str(&opts.from, browser::twitter::SEARCH_DATE_FMT)?;
    let to = NaiveDate::parse_from_str(&opts.to, browser::twitter::SEARCH_DATE_FMT)?;

    let stream =
        browser::twitter::search_by_date(&mut client, &opts.screen_name, &from, &to).await?;

    let urls = stream.try_collect::<Vec<_>>().await?;

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
