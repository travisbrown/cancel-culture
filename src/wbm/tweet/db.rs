use crate::browser::twitter::parser::BrowserTweet;
use crate::util::sqlite::{SQLiteDateTime, SQLiteId};
use chrono::{DateTime, Utc};
use futures_locks::RwLock;
use rusqlite::{params, Connection, DropBehavior, OptionalExtension, Transaction};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;

const USER_SELECT: &str = "
    SELECT id
        FROM user
        WHERE twitter_id = ? AND screen_name = ? AND name = ?
";
const USER_INSERT: &str = "INSERT INTO user (twitter_id, screen_name, name) VALUES (?, ?, ?)";

const FILE_SELECT: &str = "SELECT id FROM file WHERE digest = ?";
const FILE_INSERT: &str = "INSERT INTO file (digest, primary_twitter_id) VALUES (?, ?)";

const TWEET_SELECT_BY_ID: &str = "
    SELECT parent_twitter_id, ts, user_twitter_id, screen_name, name, content, digest
        FROM tweet
        JOIN tweet_file ON tweet_file.tweet_id = tweet.id
        JOIN file ON file.id = tweet_file.file_id
        JOIN user on user.id = tweet_file.user_id
        WHERE tweet.twitter_id = ?
        ORDER BY LENGTH(content) DESC
        LIMIT 1
";

const TWEET_MULTI_SELECT_BY_ID: &str = "
    SELECT parent_twitter_id, ts, user_twitter_id, screen_name, name, content, digest
        FROM tweet
        JOIN tweet_file ON tweet_file.tweet_id = tweet.id
        JOIN file ON file.id = tweet_file.file_id
        JOIN user on user.id = tweet_file.user_id
        WHERE tweet.twitter_id = ?
";

const TWEET_SELECT_FULL: &str = "
    SELECT id
        FROM tweet
        WHERE twitter_id = ? AND parent_twitter_id = ? AND ts = ? AND user_twitter_id = ? AND content = ?
";

const TWEET_INSERT: &str =
    "INSERT INTO tweet (twitter_id, parent_twitter_id, ts, user_twitter_id, content) VALUES (?, ?, ?, ?, ?)";

const TWEET_FILE_INSERT: &str =
    "INSERT INTO tweet_file (tweet_id, file_id, user_id) VALUES (?, ?, ?)";

const GET_USER_NAMES: &str = "
   SELECT screen_name, name
       FROM user
       WHERE twitter_id = ?
";

const GET_USER_KNOWN_ACTIVE_RANGE: &str = "
    SELECT COUNT(DISTINCT tweet.twitter_id), MIN(tweet.ts), MAX(tweet.ts)
        FROM user
        JOIN tweet_file ON tweet_file.user_id = user.id
        JOIN tweet ON tweet.id = tweet_file.tweet_id AND tweet.user_twitter_id = user.twitter_id
        WHERE user.twitter_id = ? AND user.screen_name LIKE ?;
";

const GET_REPLIES: &str = "
    SELECT tweet.twitter_id, reply_tweet.twitter_id, reply_user.twitter_id, reply_user.screen_name FROM tweet
        JOIN tweet_file ON tweet_file.tweet_id = tweet.id
        JOIN user ON user.id = tweet_file.user_id
        JOIN tweet AS reply_tweet ON reply_tweet.parent_twitter_id = tweet.twitter_id
        JOIN tweet_file AS reply_tweet_file ON reply_tweet_file.tweet_id = reply_tweet.id
        JOIN user as reply_user ON reply_user.id = reply_tweet_file.user_id
        WHERE tweet.twitter_id != reply_tweet.twitter_id AND user.twitter_id = ? AND user.screen_name like ?;
";

const GET_REPLIES_TO: &str = "
    SELECT
        tweet.twitter_id, tweet.ts, tweet.user_twitter_id, reply_tweet.twitter_id, reply_tweet.ts, reply_tweet.user_twitter_id
        FROM tweet
        JOIN tweet AS reply_tweet ON reply_tweet.parent_twitter_id = tweet.twitter_id AND reply_tweet.twitter_id != tweet.twitter_id
        WHERE tweet.user_twitter_id = ?;
";

const GET_REPLIES_FROM: &str = "
    SELECT
        tweet.twitter_id, tweet.ts, tweet.user_twitter_id, reply_tweet.twitter_id, reply_tweet.ts, reply_tweet.user_twitter_id
        FROM tweet
        JOIN tweet AS reply_tweet ON reply_tweet.parent_twitter_id = tweet.twitter_id AND reply_tweet.twitter_id != tweet.twitter_id
        WHERE reply_tweet.user_twitter_id = ?;
";

const GET_MOST_COMMON_SCREEN_NAME: &str = "
    SELECT screen_name, COUNT(tweet_file.file_id) AS c
        FROM user
        JOIN tweet_file ON user_id = user.id
        WHERE twitter_id = ?
        GROUP BY screen_name
        ORDER BY c DESC
        LIMIT 1;
";

const GET_USER_TWEETS: &str = "
    SELECT DISTINCT tweet.twitter_id, tweet.ts / 1000 AS ts, user_twitter_id, screen_name, content
        FROM tweet
        JOIN tweet_file ON tweet_id = tweet.id
        JOIN user ON user.id = user_id
        WHERE user_twitter_id = ?
        AND tweet.ts >= ? and tweet.ts <= ?;
";

pub type TweetStoreResult<T> = Result<T, TweetStoreError>;

#[derive(thiserror::Error, Debug)]
pub enum TweetStoreError {
    #[error("Missing file for TweetStore")]
    FileMissing(#[from] std::io::Error),
    #[error("SQLite error for TweetStore")]
    DbFailure(#[from] rusqlite::Error),
}

#[derive(Debug, Eq, PartialEq)]
pub struct UserRecord {
    pub id: u64,
    pub screen_name: String,
    pub names: Vec<String>,
    pub tweet_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

impl Ord for UserRecord {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.id,
            self.last_seen,
            self.first_seen,
            &self.screen_name,
            &self.names,
            self.tweet_count,
        )
            .cmp(&(
                other.id,
                other.last_seen,
                other.first_seen,
                &other.screen_name,
                &other.names,
                other.tweet_count,
            ))
    }
}

impl PartialOrd for UserRecord {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
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
                tx.execute("DROP TABLE IF EXISTS user", [])?;
                tx.execute("DROP TABLE IF EXISTS file", [])?;
                tx.execute("DROP TABLE IF EXISTS tweet_file", [])?;
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

    pub async fn get_tweet(
        &self,
        status_ids: &[u64],
    ) -> TweetStoreResult<Vec<(BrowserTweet, String)>> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare_cached(TWEET_SELECT_BY_ID)?;
        let mut result = Vec::with_capacity(status_ids.len());

        for id in status_ids {
            match select.query_row(params![SQLiteId(*id)], |row| {
                let parent_twitter_id = row.get::<usize, i64>(0)? as u64;
                let ts: SQLiteDateTime = row.get(1)?;
                let user_twitter_id = row.get::<usize, i64>(2)? as u64;
                let screen_name: String = row.get(3)?;
                let name: String = row.get(4)?;
                let content: String = row.get(5)?;
                let digest: String = row.get(6)?;

                Ok((
                    BrowserTweet::new(
                        *id,
                        if parent_twitter_id == *id {
                            None
                        } else {
                            Some(parent_twitter_id)
                        },
                        ts.0,
                        user_twitter_id,
                        screen_name,
                        name,
                        content,
                    ),
                    digest,
                ))
            }) {
                Ok(pair) => result.push(pair),
                Err(error) => log::error!("Error for {}: {:?}", id, error),
            }
        }

        Ok(result)
    }

    pub async fn get_multi_tweets(
        &self,
        status_ids: &[u64],
    ) -> TweetStoreResult<Vec<(BrowserTweet, String)>> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare_cached(TWEET_MULTI_SELECT_BY_ID)?;
        let mut result = Vec::with_capacity(status_ids.len());

        for id in status_ids {
            match select.query_and_then(params![SQLiteId(*id)], |row| {
                let parent_twitter_id = row.get::<usize, i64>(0)? as u64;
                let ts: SQLiteDateTime = row.get(1)?;
                let user_twitter_id = row.get::<usize, i64>(2)? as u64;
                let screen_name: String = row.get(3)?;
                let name: String = row.get(4)?;
                let content: String = row.get(5)?;
                let digest: String = row.get(6)?;

                Ok((
                    BrowserTweet::new(
                        *id,
                        if parent_twitter_id == *id {
                            None
                        } else {
                            Some(parent_twitter_id)
                        },
                        ts.0,
                        user_twitter_id,
                        screen_name,
                        name,
                        content,
                    ),
                    digest,
                ))
            }) {
                Ok(pairs) => result.extend(
                    pairs
                        .into_iter()
                        .collect::<Result<Vec<_>, TweetStoreError>>()?,
                ),
                Err(error) => log::error!("Error for {}: {:?}", id, error),
            }
        }

        result.sort();

        Ok(result)
    }

    pub async fn get_replies(
        &self,
        twitter_id: u64,
        screen_name: &str,
    ) -> TweetStoreResult<Vec<(u64, u64, u64, String)>> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare_cached(GET_REPLIES)?;

        let mut result: Vec<_> = select
            .query_and_then(params![SQLiteId(twitter_id), screen_name], |row| {
                let parent_twitter_id = row.get::<usize, i64>(0)? as u64;
                let status_twitter_id = row.get::<usize, i64>(1)? as u64;
                let user_twitter_id = row.get::<usize, i64>(2)? as u64;
                let screen_name: String = row.get(3)?;

                Ok((
                    parent_twitter_id,
                    status_twitter_id,
                    user_twitter_id,
                    screen_name,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;

        result.sort();
        result.dedup();

        Ok(result)
    }

    pub async fn for_each_interaction<F: Fn((u64, u64, u64, String), (u64, u64, u64, String))>(
        &self,
        twitter_id: u64,
        f: F,
    ) -> TweetStoreResult<()> {
        let connection = self.connection.read().await;
        let mut select = connection.prepare_cached(GET_REPLIES_TO)?;

        let mut seen = std::collections::HashSet::<u64>::new();

        let results = select.query_and_then(params![SQLiteId(twitter_id)], |row| {
            let reply_twitter_id = row.get::<usize, i64>(3)? as u64;

            let result: TweetStoreResult<_> = if seen.contains(&reply_twitter_id) {
                Ok(None)
            } else {
                seen.insert(reply_twitter_id);
                let twitter_id = row.get::<usize, i64>(0)? as u64;
                let twitter_ts = row.get::<usize, i64>(1)? as u64;
                let user_twitter_id = row.get::<usize, i64>(2)? as u64;
                //let screen_name: String = row.get(3)?;

                let reply_twitter_ts = row.get::<usize, i64>(4)? as u64;
                let reply_user_twitter_id = row.get::<usize, i64>(5)? as u64;
                //let reply_screen_name: String = row.get(7)?;

                Ok(Some((
                    (twitter_id, twitter_ts, user_twitter_id, "".to_string()),
                    (
                        reply_twitter_id,
                        reply_twitter_ts,
                        reply_user_twitter_id,
                        "".to_string(),
                    ),
                )))
            };

            result
        })?;

        for result in results {
            if let Some((info, reply_info)) = result? {
                f(info, reply_info);
            }
        }

        let mut select = connection.prepare_cached(GET_REPLIES_FROM)?;

        let results = select.query_and_then(params![SQLiteId(twitter_id)], |row| {
            let reply_twitter_id = row.get::<usize, i64>(3)? as u64;

            let result: TweetStoreResult<_> = if seen.contains(&reply_twitter_id) {
                Ok(None)
            } else {
                seen.insert(reply_twitter_id);
                let twitter_id = row.get::<usize, i64>(0)? as u64;
                let twitter_ts = row.get::<usize, i64>(1)? as u64;
                let user_twitter_id = row.get::<usize, i64>(2)? as u64;
                //let screen_name: String = row.get(3)?;

                let reply_twitter_ts = row.get::<usize, i64>(4)? as u64;
                let reply_user_twitter_id = row.get::<usize, i64>(5)? as u64;
                //let reply_screen_name: String = row.get(7)?;

                Ok(Some((
                    (twitter_id, twitter_ts, user_twitter_id, "".to_string()),
                    (
                        reply_twitter_id,
                        reply_twitter_ts,
                        reply_user_twitter_id,
                        "".to_string(),
                    ),
                )))
            };

            result
        })?;

        for result in results {
            if let Some((info, reply_info)) = result? {
                f(info, reply_info);
            }
        }

        Ok(())
    }

    pub async fn get_tweets_for_user(
        &self,
        user_id: u64,
        start_ts: u64,
        end_ts: u64,
    ) -> TweetStoreResult<Vec<(u64, u64, u64, String, String)>> {
        let connection = self.connection.read().await;
        let mut stmt = connection.prepare_cached(GET_USER_TWEETS)?;

        let tweets = stmt
            .query_map(
                params![SQLiteId(user_id), SQLiteId(start_ts), SQLiteId(end_ts)],
                |row| {
                    let twitter_id = row.get::<usize, i64>(0)? as u64;
                    let ts = row.get::<usize, i64>(1)? as u64;
                    let user_twitter_id = row.get::<usize, i64>(2)? as u64;
                    let screen_name = row.get(3)?;
                    let contents = row.get(4)?;
                    Ok((twitter_id, ts, user_twitter_id, screen_name, contents))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tweets)
    }

    pub async fn get_most_common_screen_names(
        &self,
        user_ids: &[u64],
    ) -> TweetStoreResult<HashMap<u64, Option<String>>> {
        let connection = self.connection.read().await;
        let mut stmt = connection.prepare_cached(GET_MOST_COMMON_SCREEN_NAME)?;
        let mut result = std::collections::HashMap::new();

        for (i, id) in user_ids.iter().enumerate() {
            if i % 100000 == 0 {
                log::info!("{}", i);
            }

            let screen_name = stmt
                .query_row(params![SQLiteId(*id)], |row| row.get(0))
                .optional()?;
            result.insert(*id, screen_name);
        }

        Ok(result)
    }

    pub async fn get_users(&self, user_ids: &[u64]) -> TweetStoreResult<Vec<UserRecord>> {
        let connection = self.connection.read().await;
        let mut get_user_names = connection.prepare_cached(GET_USER_NAMES)?;
        let mut get_user_known_active_range =
            connection.prepare_cached(GET_USER_KNOWN_ACTIVE_RANGE)?;
        let mut result = Vec::with_capacity(user_ids.len());

        for (i, id) in user_ids.iter().enumerate() {
            if i % 1000 == 0 {
                log::info!("{}", i);
            }

            let pairs: Vec<(String, String)> = get_user_names
                .query_map(params![SQLiteId(*id)], |row| {
                    let screen_name: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    Ok((screen_name, name))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut name_map = HashMap::<String, Vec<String>>::new();
            for (screen_name, name) in pairs {
                if let Some((_, names)) = name_map.iter_mut().find(|(known_screen_name, _)| {
                    known_screen_name.to_lowercase() == screen_name.to_lowercase()
                }) {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                } else {
                    name_map.insert(screen_name, vec![name]);
                }
            }

            let snc = name_map.len();
            let nnc = name_map.values().flat_map(|v| v.iter()).count();

            if snc > 1 || nnc > 1 {
                //log::info!("{} {}", snc, nnc);
            }

            for (screen_name, names) in name_map {
                match get_user_known_active_range.query_row(
                    params![SQLiteId(*id), screen_name.clone()],
                    |row| {
                        let tweet_count = row.get(0)?;
                        let first: SQLiteDateTime = row.get(1)?;
                        let last: SQLiteDateTime = row.get(2)?;

                        Ok(UserRecord {
                            id: *id,
                            screen_name: screen_name.clone(),
                            names: names.clone(),
                            tweet_count,
                            first_seen: first.0,
                            last_seen: last.0,
                        })
                    },
                ) {
                    Err(error) => {
                        log::error!(
                            "Error retrieving user date range for {} ({}): {:?}",
                            id,
                            screen_name,
                            error
                        )
                    }
                    Ok(record) => result.push(record),
                }
            }
        }

        Ok(result)
    }

    pub async fn tweet_ids_by_user_id(&self, user_id: u64) -> TweetStoreResult<Vec<u64>> {
        let connection = self.connection.read().await;
        let mut select = connection
            .prepare_cached("SELECT tweet.twitter_id FROM tweet WHERE user_twitter_id = ?")?;

        let result = select
            .query_map(params![user_id], |row| Ok(row.get::<usize, i64>(0)? as u64))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(result)
    }

    pub async fn check_linkable(&self, digests_path: &str) -> TweetStoreResult<()> {
        use std::io::{self, BufRead};
        let file = std::fs::File::open(digests_path)?;
        let digests: std::collections::HashSet<String> =
            io::BufReader::new(file).lines().collect::<Result<_, _>>()?;

        let connection = self.connection.read().await;
        let mut select = connection.prepare_cached(
            "SELECT tweet.twitter_id, file.digest FROM tweet_file JOIN tweet ON tweet.id = tweet_id JOIN file ON file.id = file_id ORDER BY tweet.twitter_id"
        )?;

        let mut current_twitter_id = 0;
        let mut current_good = 0;
        let mut current_bad = vec![];

        select
            .query_map(params![], |row| {
                let twitter_id = row.get::<usize, i64>(0)? as u64;
                let digest: String = row.get(1)?;

                if twitter_id != current_twitter_id {
                    if current_twitter_id != 0 {
                        println!(
                            "{},{},{},{}",
                            current_twitter_id,
                            current_good,
                            current_bad.len(),
                            current_bad.join(";")
                        );
                    }

                    current_twitter_id = twitter_id;
                    current_good = 0;
                    current_bad.clear();
                }

                if digests.contains(&digest) {
                    current_good += 1;
                } else {
                    current_bad.push(digest);
                }

                Ok(())
            })?
            .collect::<Result<Vec<_>, _>>()?;

        println!(
            "{},{},{},{}",
            current_twitter_id,
            current_good,
            current_bad.len(),
            current_bad.join(";")
        );

        Ok(())
    }
}
