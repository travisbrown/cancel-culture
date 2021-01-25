use cancel_culture::{cli, twitter::store::wayback::TweetStore, wayback::Store};
use clap::{crate_authors, crate_version, Clap};

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    let store = Store::load(opts.store_dir)?;
    let tweet_store = TweetStore::new(opts.db, opts.clean)?;

    Ok(store.export_tweets(&tweet_store).await?)
}

#[derive(Clap)]
#[clap(name = "wbtweets", version = crate_version!(), author = crate_authors!())]
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
