use cancel_culture::{
    cli,
    wayback::{Result, Store},
};
use clap::{crate_authors, crate_version, Clap};

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    let store = Store::load(opts.store_dir)?;
    let query = opts.query;
    let tweets = store
        .extract_all_tweets(move |item| item.url.contains(&query), 8)
        .await?;

    let mut result = tweets.into_iter().collect::<Vec<_>>();
    result.sort_by_key(|(id, tweet)| (tweet[0].user_screen_name.clone(), *id));

    for (_, tweets) in result {
        for tweet in tweets {
            println!("{} {}", tweet.id, tweet.user_screen_name);
        }
    }

    Ok(())
}

#[derive(Clap)]
#[clap(name = "wbtweets", version = crate_version!(), author = crate_authors!())]
struct Opts {
    /// Wayback Machine store directory
    #[clap(short, long, default_value = "wayback")]
    store_dir: String,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    query: String,
}
