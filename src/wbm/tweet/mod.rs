pub mod db;

use super::valid::ValidStore;
use crate::browser::twitter::parser::{self, BrowserTweet};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error")]
    IOError(#[from] std::io::Error),
    #[error("TweetStore error")]
    TweetStoreError(#[from] db::TweetStoreError),
    #[error("ValidStore error")]
    ValidStoreError(#[from] super::valid::Error),
}

type Result<T> = std::result::Result<T, Error>;

fn extract_tweets_from_path<P: AsRef<Path>>(
    p: P,
) -> Result<Option<(Option<u64>, Vec<BrowserTweet>)>> {
    let path = p.as_ref();

    if path.is_file() {
        let file = File::open(path)?;
        let mut doc = String::new();
        let mut gz = GzDecoder::new(file);
        gz.read_to_string(&mut doc)?;

        Ok(match parser::extract_tweet_json(&doc) {
            Some(tweet) => Some((Some(tweet.id), vec![tweet])),
            None => match parser::parse_html(&mut doc.as_bytes()) {
                Ok(doc) => Some((
                    parser::extract_canonical_status_id(&doc),
                    parser::extract_tweets(&doc),
                )),
                Err(err) => {
                    log::error!("Failed reading {:?}: {:?}", path, err);
                    None
                }
            },
        })
    } else {
        Ok(None)
    }
}

pub async fn export_tweets(store: &ValidStore, tweet_store: &db::TweetStore) -> Result<()> {
    use futures::TryStreamExt;

    Ok(
        futures::stream::iter(store.paths().map(|result| result.map_err(Error::from)))
            .try_for_each_concurrent(4, |(digest, path)| async move {
                if tweet_store.check_digest(&digest).await?.is_none() {
                    let ts = tweet_store.clone();
                    let act = tokio::spawn(async move {
                        if let Ok(Some((status_id, tweets))) = extract_tweets_from_path(path) {
                            ts.add_tweets(&digest, status_id, &tweets).await.unwrap()
                        }
                    });

                    act.await.unwrap()
                }
                Ok(())
            })
            .await?,
    )
}
