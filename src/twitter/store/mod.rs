mod wrappers;

use chrono::{DateTime, Utc};
use egg_mode::user::TwitterUser;
use log::info;
use rusqlite::{params, Connection, Result};
use std::collections::HashMap;
use wrappers::{SQLiteDateTime, SQLiteId};

const USER_INSERT: &str = "INSERT OR IGNORE INTO user (id, ts) VALUES (?, ?)";
const SCREEN_NAME_INSERT: &str = "INSERT OR IGNORE INTO screen_name (value) VALUES (?)";
const SCREEN_NAME_SELECT: &str = "SELECT id FROM screen_name WHERE value = ?";
const FOLLOW_INSERT: &str = "INSERT OR IGNORE INTO follow (follower_id, followed_id) VALUES (?, ?)";

const USER_OBSERVATION_SELECT: &str = "
    SELECT user_id, value, follower_count
        FROM user_observation
        JOIN screen_name ON screen_name.id = user_observation.screen_name_id
        GROUP BY user_id
        HAVING ts = MAX(ts)
";

const FOLLOW_SELECT: &str = "
    SELECT * FROM (
        SELECT follower_id, MAX(ts), 1
            FROM follow
            WHERE followed_id = (:id)
            GROUP BY follower_id
            UNION ALL
                SELECT follower_id, MAX(ts), 0
                    FROM unfollow
                    WHERE followed_id = (:id)
                    GROUP BY follower_id
    ) WHERE follower_id NOT NULL
";

const NEXT_USER_SELECT: &str = "
    SELECT fid, COUNT(*) as c FROM (
        SELECT follower_id AS fid FROM follow
            UNION ALL SELECT followed_id AS fid FROM follow
    )
    LEFT JOIN user ON user.id = fid WHERE user.id IS NULL
    GROUP BY fid
    ORDER BY c DESC, fid
    LIMIT ?
";

const USER_OBSERVATION_INSERT: &str = "
    INSERT INTO user_observation (
        user_id,
        screen_name_id,
        follower_count,
        following_count,
        verified
    ) VALUES (?, ?, ?, ?, ?)
";

const TWEET_INSERT: &str = "INSERT INTO tweet (id, user_id) VALUES (?, ?)";
const TWEET_DATA_INSERT: &str = "
  INSERT INTO tweet_data (tweet_id, created, content, reply_to, retweet_of, quoting)
      VALUES (?, ?, ?, ?, ?, ?)
";
const TWEET_OBSERVATION_INSERT: &str =
    "INSERT INTO tweet_observation (tweet_id, retweet_count, favorite_count) VALUES (?, ?, ?)";

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn new(connection: Connection) -> Store {
        Store { connection }
    }

    pub fn get_next_users(&self, count: u32) -> Result<Vec<u64>> {
        let mut select = self.connection.prepare(NEXT_USER_SELECT)?;

        let res = select
            .query_map(params![count], |row| {
                row.get::<usize, i64>(0).map(|v| v as u64)
            })?
            .collect::<Result<Vec<u64>, rusqlite::Error>>();

        res
    }

    pub fn add_follows<I: IntoIterator<Item = (u64, u64)>>(&self, relations: I) -> Result<()> {
        let mut follow_insert = self.connection.prepare(FOLLOW_INSERT)?;

        for (follower_id, followed_id) in relations {
            follow_insert.execute(&[SQLiteId(follower_id), SQLiteId(followed_id)])?;
        }

        Ok(())
    }

    pub fn add_users(&self, users: &[TwitterUser]) -> Result<()> {
        let mut user_insert = self.connection.prepare(USER_INSERT)?;
        let mut screen_name_insert = self.connection.prepare(SCREEN_NAME_INSERT)?;
        let mut screen_name_select = self.connection.prepare(SCREEN_NAME_SELECT)?;
        let mut user_observation_insert = self.connection.prepare(USER_OBSERVATION_INSERT)?;

        for user in users {
            info!("Adding: {}", user.screen_name);
            let id = SQLiteId(user.id);
            user_insert.execute(params![id, SQLiteDateTime(user.created_at)])?;
            screen_name_insert.execute(params![&user.screen_name])?;

            let screen_name_id: i64 =
                screen_name_select.query_row(params![&user.screen_name], |row| row.get(0))?;

            user_observation_insert.execute(params![
                id,
                screen_name_id,
                user.followers_count,
                user.friends_count,
                user.verified
            ])?;
        }

        Ok(())
    }

    pub fn get_follower_counts(&self) -> Result<Vec<(u64, String, usize, usize)>> {
        let mut user_select = self.connection.prepare(USER_OBSERVATION_SELECT)?;

        let users = user_select
            .query_map_named(&[], |row| {
                let id = row.get::<usize, i64>(0)? as u64;
                let screen_name: String = row.get(1)?;
                let count = row.get::<usize, i64>(2)? as usize;

                Ok((id, screen_name, count))
            })?
            .collect::<Result<Vec<_>>>()?;

        let mut res = Vec::with_capacity(users.len());

        for (id, screen_name, count) in users {
            let known = self.get_followers(id)?;
            let known_count = known.iter().filter(|(_, (v, _))| *v).count();

            res.push((id, screen_name, count, known_count))
        }

        Ok(res)
    }

    pub fn get_followers(&self, id: u64) -> Result<HashMap<u64, (bool, DateTime<Utc>)>> {
        let mut follow_select = self.connection.prepare(FOLLOW_SELECT)?;

        let res = follow_select
            .query_map_named(&[(":id", &(id as i64))], |row| {
                let id = row.get::<usize, i64>(0)? as u64;
                let ts: SQLiteDateTime = row.get(1)?;
                let is_follow: bool = row.get(2)?;

                Ok((id, (is_follow, ts.0)))
            })?
            .collect();

        res
    }

    /*pub fn add_tweets(&self, tweets: &[Tweet]) -> Result<()> {
        let mut tweet_insert = self.connection.prepare(TWEET_INSERT)?;
        let mut tweet_data_insert = self.connection.prepare(TWEET_DATA_INSERT)?;
        let mut tweet_observation_insert = self.connection.prepare(TWEET_OBSERVATION_INSERT)?;

        for tweet in tweets {
            info!("Adding: {}", tweet.id);
            let tweet_id = SQLiteId(tweet.id);
            let user_id = SQLite(tweet.user.expect("User tweets are not supported").id)
            tweet_insert.execute(params![tweet_id, user_id])?;

            tweet_data_insert.execute(params![&user.screen_name])?;

            let screen_name_id: i64 =
                screen_name_select.query_row(params![&user.screen_name], |row| row.get(0))?;

            user_observation_insert.execute(params![
                id,
                screen_name_id,
                user.followers_count,
                user.friends_count,
                user.verified
            ])?;
        }

        Ok(())
    }*/
}
