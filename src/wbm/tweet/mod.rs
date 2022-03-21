pub mod db;

use super::valid::ValidStore;
use crate::browser::twitter::parser::{self, BrowserTweet};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error")]
    IOError(#[from] std::io::Error),
    #[error("TweetStore error")]
    TweetStoreError(#[from] db::TweetStoreError),
    #[error("ValidStore error")]
    ValidStoreError(#[from] super::valid::Error),
    #[error("Task error")]
    Task(#[from] tokio::task::JoinError),
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
    use futures::{FutureExt, StreamExt, TryStreamExt};

    futures::stream::iter(store.paths().map(|result| result.map_err(Error::from)))
        .filter_map(|res| async {
            match res {
                Err(Error::ValidStoreError(super::valid::Error::Unexpected { path: _ })) => None,
                other => Some(other),
            }
        })
        .try_filter_map(|(digest, path)| {
            let digest_clone = digest.clone();
            async move {
                if tweet_store.check_digest(&digest).await?.is_none() {
                    Ok(Some(
                        tokio::task::spawn(async move {
                            extract_tweets_from_path(path).map(|outer_option| {
                                outer_option
                                    .map(|(status_id, tweets)| (digest_clone, status_id, tweets))
                            })
                        })
                        .then(move |res| async move {
                            match res {
                                Ok(Err(Error::IOError(underlying))) => {
                                    log::warn!("Error parsing {}: {:?}", digest, underlying);
                                    Ok(None)
                                }
                                Ok(inner_res) => inner_res,
                                Err(error) => Err(Error::from(error)),
                            }
                        }),
                    ))
                } else {
                    Ok(None)
                }
            }
        })
        .try_buffer_unordered(4)
        .try_filter_map(|maybe_content| async { Ok(maybe_content) })
        .try_for_each(|(digest, status_id, tweets)| async move {
            tweet_store.add_tweets(&digest, status_id, &tweets).await?;
            Ok(())
        })
        .await
}
