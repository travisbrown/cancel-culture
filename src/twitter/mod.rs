use egg_mode::user::{TwitterUser, UserID};
use egg_mode::{KeyPair, Token};
use fantoccini::Client as FClient;
use futures::TryStreamExt;
use regex::Regex;
use serde_derive::Deserialize;
use std::collections::HashMap;
use std::default::Default;
use std::fs;
use std::mem::drop;
use std::result;
use std::sync::{Arc, RwLock};

#[derive(Deserialize)]
struct Config {
    twitter: TwitterConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TwitterConfig {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_token_secret: String,
}

impl TwitterConfig {
    pub fn key_pairs(self) -> (KeyPair, KeyPair) {
        (
            KeyPair::new(self.consumer_key, self.consumer_secret),
            KeyPair::new(self.access_token, self.access_token_secret),
        )
    }
}

pub type Result<T> = std::result::Result<T, Error>;
pub type EggModeResult<T> = std::result::Result<T, egg_mode::error::Error>;

#[derive(Debug)]
pub enum Error {
    ConfigParseError(toml::de::Error),
    ConfigReadError(std::io::Error),
    ApiError(egg_mode::error::Error),
    BrowserError(fantoccini::error::CmdError),
    HttpClientError(reqwest::Error),
    TweetIDParseError(String),
    NotReplyError(u64),
    MissingUserError(u64),
}

impl From<toml::de::Error> for Error {
    fn from(e: toml::de::Error) -> Self {
        Error::ConfigParseError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::ConfigReadError(e)
    }
}

impl From<egg_mode::error::Error> for Error {
    fn from(e: egg_mode::error::Error) -> Self {
        Error::ApiError(e)
    }
}

impl From<fantoccini::error::CmdError> for Error {
    fn from(e: fantoccini::error::CmdError) -> Self {
        Error::BrowserError(e)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::HttpClientError(e)
    }
}

#[derive(Default)]
struct UserCache {
    by_id: HashMap<u64, TwitterUser>,
    id_by_screen_name: HashMap<String, u64>,
    screen_name_by_id: HashMap<u64, String>,
}

pub struct Client {
    token: Token,
    user: TwitterUser,
    user_cache: Arc<RwLock<UserCache>>,
}

impl Client {
    fn new(token: Token, user: TwitterUser) -> Client {
        let mut user_cache = UserCache::default();

        user_cache
            .id_by_screen_name
            .insert(user.screen_name.clone(), user.id);
        user_cache
            .screen_name_by_id
            .insert(user.id, user.screen_name.clone());
        user_cache.by_id.insert(user.id, user.clone());

        Client {
            token,
            user,
            user_cache: Arc::new(RwLock::new(user_cache)),
        }
    }

    pub async fn from_key_pairs(
        consumer: KeyPair,
        access: KeyPair,
    ) -> result::Result<Client, egg_mode::error::Error> {
        let token = Token::Access { consumer, access };
        let user = egg_mode::auth::verify_tokens(&token).await?.response;
        Ok(Client::new(token, user))
    }

    pub async fn from_config_file(path: &str) -> Result<Client> {
        let contents = fs::read_to_string(path)?;
        let config = toml::from_str::<Config>(&contents)?;
        let (consumer, access) = config.twitter.key_pairs();

        Ok(Self::from_key_pairs(consumer, access).await?)
    }

    pub fn parse_tweet_id(input: &str) -> Result<u64> {
        input
            .split('/')
            .last()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| Error::TweetIDParseError(input.to_string()))
    }

    pub async fn blocks(&self) -> EggModeResult<Vec<u64>> {
        egg_mode::user::blocks_ids(&self.token)
            .map_ok(|r| r.response)
            .try_collect()
            .await
    }

    pub async fn friends<T: Into<UserID>>(&self, acct: T) -> EggModeResult<Vec<u64>> {
        egg_mode::user::friends_ids(acct, &self.token)
            .with_page_size(5000)
            .map_ok(|r| r.response)
            .try_collect()
            .await
    }

    pub async fn friends_self(&self) -> EggModeResult<Vec<u64>> {
        self.friends(self.user.id).await
    }

    pub async fn followers<T: Into<UserID>>(&self, acct: T) -> EggModeResult<Vec<u64>> {
        egg_mode::user::followers_ids(acct, &self.token)
            .with_page_size(5000)
            .map_ok(|r| r.response)
            .try_collect()
            .await
    }

    pub async fn followers_self(&self) -> EggModeResult<Vec<u64>> {
        self.followers(self.user.id).await
    }

    pub async fn get_in_reply_to(&self, id: u64) -> EggModeResult<Option<(String, u64)>> {
        let res = egg_mode::tweet::lookup(vec![id], &self.token).await?;
        let tweet = res.response.get(0);

        Ok(tweet.and_then(|t| {
            t.in_reply_to_screen_name
                .as_ref()
                .cloned()
                .zip(t.in_reply_to_status_id)
        }))
    }

    pub async fn statuses_exist<I: IntoIterator<Item = u64>>(
        &self,
        ids: I,
        fallback_client: Option<&mut FClient>,
    ) -> Result<HashMap<u64, bool>> {
        let mut status_map = HashMap::new();
        let ids_vec = ids.into_iter().collect::<Vec<u64>>();
        let chunks = ids_vec.chunks(100);

        for chunk in chunks {
            let m = egg_mode::tweet::lookup_map(chunk.to_vec(), &self.token)
                .await?
                .response;

            for (k, v) in m {
                status_map.insert(k, v);
            }
        }

        match fallback_client {
            Some(client) => {
                let mut res = HashMap::new();

                for (k, v) in status_map.iter() {
                    if v.is_some() {
                        res.insert(*k, true);
                    } else {
                        res.insert(
                            *k,
                            crate::browser::twitter::status_exists(client, *k).await?,
                        );
                    }
                }

                Ok(res)
            }
            None => Ok(status_map.iter().map(|(k, v)| (*k, v.is_some())).collect()),
        }
    }

    pub async fn lookup_users(&self, ids: &[u64]) -> Result<Vec<TwitterUser>> {
        let cache = self.user_cache.read().unwrap();

        let unknown_ids = ids
            .iter()
            .cloned()
            .filter(|id| !cache.by_id.contains_key(id))
            .collect::<Vec<u64>>();
        drop(cache);

        let chunks = unknown_ids.chunks(100);
        let mut new_users = Vec::with_capacity(unknown_ids.len());

        for chunk in chunks {
            let batch = egg_mode::user::lookup(chunk.to_vec(), &self.token)
                .await?
                .response;
            new_users.extend(batch);
        }

        let mut writeable_cache = self.user_cache.write().unwrap();

        for user in new_users {
            writeable_cache
                .id_by_screen_name
                .insert(user.screen_name.clone(), user.id);
            writeable_cache.by_id.insert(user.id, user);
        }

        let mut res = Vec::with_capacity(ids.len());

        for id in ids {
            let user = writeable_cache
                .by_id
                .get(id)
                .ok_or(Error::MissingUserError(*id))?;
            res.push(user.clone());
        }

        Ok(res)
    }
}

const STATUS_PATTERN: &str = r"^http[s]?://twitter\.com/[^/]+/status/(\d+)(?:\?.+)?$";

pub fn extract_status_id(url: &str) -> Option<u64> {
    Regex::new(STATUS_PATTERN).ok().and_then(|re| {
        re.captures(url)
            .and_then(|groups| groups.get(1).and_then(|m| m.as_str().parse::<u64>().ok()))
    })
}
