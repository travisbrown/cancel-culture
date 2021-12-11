use cancel_culture::{cli, twitter::store::wayback::TweetStore, wayback::Store};
use clap::Parser;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose)?;

    let store = Store::load(opts.store_dir)?;
    let tweet_store = TweetStore::new(opts.db, opts.clean)?;

    store.export_tweets(&tweet_store).await?;

    log::logger().flush();

    Ok(())
}

#[derive(Parser)]
#[clap(name = "wbtweets", version, author)]
struct Opts {
    /// Wayback Machine store directory
    #[clap(short, long, default_value = "wayback")]
    store_dir: String,
    /// Wayback Machine tweet store database file
    #[clap(short, long, default_value = "wb-tweets.db")]
    db: String,
    /// Reset database
    #[clap(long)]
    clean: bool,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
}
