use crate::browser::twitter::parser::BrowserTweet;
use crate::util::sqlite::{SQLiteDateTime, SQLiteId};
use crate::wayback::Item;
use futures_locks::RwLock;
use rusqlite::{params, Connection, DropBehavior, OptionalExtension};
use std::collections::HashSet;
use std::path::Path;
use thiserror::Error;

const DIGEST_SELECT: &str = "SELECT DISTINCT value FROM digest";
const DIGEST_INSERT: &str = "INSERT OR IGNORE INTO digest (tweet_id, value, url) VALUES (?, ?, ?)";

const TWEET_INSERT: &str =
    "INSERT INTO tweet (twitter_id, ts, user_id, user_screen_name, content) VALUES (?, ?, ?, ?, ?)";
const TWEET_SELECT: &str = "
    SELECT id
        FROM tweet
        WHERE twitter_id = ?
        AND ts = ?
        AND user_id = ?
        AND user_screen_name = ?
        AND content = ?
        LIMIT 1
";

const TWEET_SELECT_WITH_DIGEST: &str = "
    SELECT twitter_id, ts, user_id, user_screen_name, content, value
        FROM tweet
        JOIN digest ON digest.tweet_id = tweet.id
        WHERE twitter_id = ?;
";

type TweetStoreResult<T> = Result<T, TweetStoreError>;

#[derive(Error, Debug)]
pub enum TweetStoreError {
    #[error("missing file for TweetStore")]
    FileMissing(#[from] std::io::Error),
    #[error("SQLite error for TweetStore")]
    DbFailure(#[from] rusqlite::Error),
}

#[derive(Clone)]
pub struct TweetStore {
    connection: RwLock<Connection>,
}

impl TweetStore {
    pub fn new<P: AsRef<Path>>(path: P, recreate: bool) -> TweetStoreResult<TweetStore> {
        let exists = path.as_ref().is_file();
        let mut connection = Connection::open(path)?;

        if exists {
            if recreate {
                let tx = connection.transaction()?;
                tx.execute("DROP TABLE IF EXISTS tweet", [])?;
                tx.execute("DROP TABLE IF EXISTS digest", [])?;
                let schema = Self::load_schema()?;
                tx.execute_batch(&schema)?;
                tx.commit()?;
            }
        } else {
            let schema = Self::load_schema()?;
            connection.execute_batch(&schema)?;
        }

        Ok(TweetStore {
            connection: RwLock::new(connection),
        })
    }

    fn load_schema() -> std::io::Result<String> {
        std::fs::read_to_string("schemas/wb-tweet.sql")
    }

    pub async fn get_known_digests(&self) -> TweetStoreResult<HashSet<String>> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare(DIGEST_SELECT)?;
        let row = select
            .query_map([], |row| row.get(0))?
            .collect::<Result<HashSet<String>, rusqlite::Error>>()?;
        Ok(row)
    }

    pub async fn lookup_tweet(&self, id: u64) -> TweetStoreResult<Vec<(BrowserTweet, String)>> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare(TWEET_SELECT_WITH_DIGEST)?;
        let rows = select
            .query_map(params![SQLiteId(id)], move |row| {
                let id = row.get::<usize, i64>(0)? as u64;
                let time: SQLiteDateTime = row.get(1)?;
                let user_id = row.get::<usize, i64>(2)? as u64;
                let user_screen_name = row.get(3)?;
                let content = row.get(4)?;
                let digest = row.get(5)?;
                Ok((
                    BrowserTweet::new(id, time.0, user_id, user_screen_name, content),
                    digest,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;

        Ok(rows)
    }

    pub async fn add_tweets(&self, tweets: &[BrowserTweet], item: &Item) -> TweetStoreResult<()> {
        let mut connection = self.connection.write().await;
        let mut tx = connection.transaction()?;
        tx.set_drop_behavior(DropBehavior::Commit);

        let mut select = tx.prepare_cached(TWEET_SELECT)?;
        let mut insert_tweet = tx.prepare_cached(TWEET_INSERT)?;
        let mut insert_digest = tx.prepare_cached(DIGEST_INSERT)?;

        for tweet in tweets {
            let current_id: Option<i64> = select
                .query_row(
                    params![
                        SQLiteId(tweet.id),
                        SQLiteDateTime(tweet.time),
                        SQLiteId(tweet.user_id),
                        tweet.user_screen_name,
                        tweet.text
                    ],
                    |row| row.get(0),
                )
                .optional()?;

            let tweet_id = match current_id {
                None => {
                    insert_tweet.execute(params![
                        SQLiteId(tweet.id),
                        SQLiteDateTime(tweet.time),
                        SQLiteId(tweet.user_id),
                        tweet.user_screen_name,
                        tweet.text
                    ])?;

                    tx.last_insert_rowid()
                }
                Some(id) => id,
            };

            insert_digest.execute(params![tweet_id, item.digest, item.wayback_url(false)])?;
        }

        Ok(())
    }
}
