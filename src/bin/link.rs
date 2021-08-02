use cancel_culture::wbm::tweet::db::TweetStore;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let args: Vec<String> = std::env::args().collect();
    let db = args.get(1).unwrap();
    let digests = args.get(2).unwrap();

    let tweet_store = TweetStore::new(db, false)?;
    tweet_store.check_linkable(digests).await?;

    Ok(())
}
