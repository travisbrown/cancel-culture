use crate::browser::twitter::parser::BrowserTweet;
use crate::util::sqlite::{SQLiteDateTime, SQLiteId};
use futures_locks::RwLock;
use rusqlite::{params, Connection, DropBehavior, OptionalExtension, Transaction, NO_PARAMS};
use std::path::Path;
use thiserror::Error;

const USER_SELECT: &str = "
    SELECT id
        FROM user
        WHERE twitter_id = ? AND screen_name = ? AND name = ?
";
const USER_INSERT: &str = "INSERT INTO user (twitter_id, screen_name, name) VALUES (?, ?, ?)";

const FILE_SELECT: &str = "SELECT id FROM file WHERE digest = ?";
const FILE_INSERT: &str = "INSERT INTO file (digest, primary_twitter_id) VALUES (?, ?)";

const TWEET_SELECT_FULL: &str = "
    SELECT id
        FROM tweet
        WHERE twitter_id = ? AND parent_twitter_id = ? AND ts = ? AND user_twitter_id = ? AND content = ?
";
const TWEET_INSERT: &str =
    "INSERT INTO tweet (twitter_id, parent_twitter_id, ts, user_twitter_id, content) VALUES (?, ?, ?, ?, ?)";

const TWEET_FILE_INSERT: &str =
    "INSERT INTO tweet_file (tweet_id, file_id, user_id) VALUES (?, ?, ?)";

pub type TweetStoreResult<T> = Result<T, TweetStoreError>;

#[derive(Error, Debug)]
pub enum TweetStoreError {
    #[error("Missing file for TweetStore")]
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
                tx.execute("DROP TABLE IF EXISTS tweet", NO_PARAMS)?;
                tx.execute("DROP TABLE IF EXISTS user", NO_PARAMS)?;
                tx.execute("DROP TABLE IF EXISTS file", NO_PARAMS)?;
                tx.execute("DROP TABLE IF EXISTS tweet_file", NO_PARAMS)?;
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

    pub async fn check_digest(&self, digest: &str) -> TweetStoreResult<Option<i64>> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare_cached(FILE_SELECT)?;

        Ok(select
            .query_row(params![digest], |row| row.get(0))
            .optional()?)
    }

    pub async fn add_tweets(
        &self,
        digest: &str,
        primary_twitter_id: Option<u64>,
        tweets: &[BrowserTweet],
    ) -> TweetStoreResult<()> {
        let mut connection = self.connection.write().await;
        let mut tx = connection.transaction()?;
        tx.set_drop_behavior(DropBehavior::Commit);

        let mut insert_file = tx.prepare_cached(FILE_INSERT)?;
        insert_file.execute(params![digest, primary_twitter_id.map(SQLiteId)])?;
        let file_id = tx.last_insert_rowid();

        let mut select_tweet = tx.prepare_cached(TWEET_SELECT_FULL)?;
        let mut insert_tweet = tx.prepare_cached(TWEET_INSERT)?;
        let mut insert_tweet_file = tx.prepare_cached(TWEET_FILE_INSERT)?;

        for tweet in tweets {
            let user_id = Self::add_user(
                &tx,
                tweet.user_id,
                &tweet.user_screen_name,
                &tweet.user_name,
            )?;

            let existing_id: Option<i64> = select_tweet
                .query_row(
                    params![
                        SQLiteId(tweet.id),
                        SQLiteId(tweet.parent_id.unwrap_or(tweet.id)),
                        SQLiteDateTime(tweet.time),
                        SQLiteId(tweet.user_id),
                        tweet.text
                    ],
                    |row| row.get(0),
                )
                .optional()?;

            let tweet_id = match existing_id {
                None => {
                    insert_tweet.execute(params![
                        SQLiteId(tweet.id),
                        SQLiteId(tweet.parent_id.unwrap_or(tweet.id)),
                        SQLiteDateTime(tweet.time),
                        SQLiteId(tweet.user_id),
                        tweet.text
                    ])?;

                    tx.last_insert_rowid()
                }
                Some(id) => id,
            };

            insert_tweet_file.execute(params![tweet_id, file_id, user_id])?;
        }

        Ok(())
    }

    fn load_schema() -> std::io::Result<String> {
        std::fs::read_to_string("schemas/tweet.sql")
    }

    fn add_user(
        tx: &Transaction,
        twitter_id: u64,
        screen_name: &str,
        name: &str,
    ) -> TweetStoreResult<i64> {
        let mut select = tx.prepare_cached(USER_SELECT)?;
        let id = match select
            .query_row(params![SQLiteId(twitter_id), screen_name, name], |row| {
                row.get(0)
            })
            .optional()?
        {
            Some(id) => id,
            None => {
                let mut insert = tx.prepare_cached(USER_INSERT)?;
                insert.execute(params![SQLiteId(twitter_id), screen_name, name])?;
                tx.last_insert_rowid()
            }
        };
        Ok(id)
    }
}

/*
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
*/

/*pub async fn get_known_digests(&self) -> TweetStoreResult<HashSet<String>> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare(DIGEST_SELECT)?;
        let row = select
            .query_map(NO_PARAMS, |row| row.get(0))?
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

    pub async fn add_tweets(&self, tweets: &[BrowserTweet]) -> TweetStoreResult<()> {
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
*/
