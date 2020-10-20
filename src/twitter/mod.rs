pub mod config;
mod error;
mod method;
mod rate_limited;
pub mod store;
pub mod timeline;

pub use error::Error;
pub use method::Method;

use egg_mode::cursor::{CursorIter, IDCursor};
use egg_mode::tweet::Tweet;
use egg_mode::user::{TwitterUser, UserID};
use egg_mode::{KeyPair, Response, Token};
use futures::future::try_join_all;
use futures::{Future, FutureExt, Stream, TryStreamExt};
use rate_limited::{RateLimitedClient, RateLimitedStream, TimelineScrollback};
use regex::Regex;
use std::collections::HashMap;
use std::default::Default;
use std::fs;
use std::mem::drop;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub type Result<T> = std::result::Result<T, Error>;
pub type EggModeResult<T> = std::result::Result<T, egg_mode::error::Error>;
type ResponseFuture<'a, T> = Pin<Box<dyn Future<Output = EggModeResult<Response<T>>> + 'a>>;

#[derive(Default)]
struct UserCache {
    by_id: HashMap<u64, TwitterUser>,
    id_by_screen_name: HashMap<String, u64>,
    screen_name_by_id: HashMap<u64, String>,
}

pub struct Client {
    token: Token,
    app_token: Token,
    user: TwitterUser,
    user_cache: Arc<RwLock<UserCache>>,
    app_limited_client: RateLimitedClient,
}

impl Client {
    async fn new(
        token: Token,
        app_token: Token,
        user: TwitterUser,
    ) -> egg_mode::error::Result<Client> {
        let mut user_cache = UserCache::default();

        user_cache
            .id_by_screen_name
            .insert(user.screen_name.clone(), user.id);
        user_cache
            .screen_name_by_id
            .insert(user.id, user.screen_name.clone());
        user_cache.by_id.insert(user.id, user.clone());

        let app_limited_client = RateLimitedClient::new(app_token.clone()).await?;

        Ok(Client {
            token,
            app_token,
            user,
            user_cache: Arc::new(RwLock::new(user_cache)),
            app_limited_client,
        })
    }

    pub async fn from_key_pairs(
        consumer: KeyPair,
        access: KeyPair,
    ) -> std::result::Result<Client, egg_mode::error::Error> {
        let app_token = egg_mode::auth::bearer_token(&consumer).await?;
        let token = Token::Access { consumer, access };
        let user = egg_mode::auth::verify_tokens(&token).await?.response;
        Client::new(token, app_token, user).await
    }

    pub async fn from_config_file<P: AsRef<Path>>(path: P) -> Result<Client> {
        let contents = fs::read_to_string(path)?;
        let config = toml::from_str::<config::Config>(&contents)?;
        let (consumer, access) = config.twitter_key_pairs();

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

    pub fn tweets<T: Into<UserID>>(
        &self,
        acct: T,
        with_replies: bool,
        with_rts: bool,
    ) -> RateLimitedStream<'static, TimelineScrollback> {
        self.app_limited_client.timeline_scrollback_stream(
            Method::USER_TIMELINE,
            egg_mode::tweet::user_timeline(acct, with_replies, with_rts, &self.app_token)
                .with_page_size(200),
        )
    }

    pub fn follower_ids<T: Into<UserID>>(
        &self,
        acct: T,
    ) -> RateLimitedStream<'static, CursorIter<IDCursor>> {
        let cursor = egg_mode::user::followers_ids(acct, &self.app_token).with_page_size(5000);

        self.app_limited_client
            .cursor_stream(Method::USER_FOLLOWER_IDS, cursor)
    }

    pub fn followed_ids<T: Into<UserID>>(
        &self,
        acct: T,
    ) -> RateLimitedStream<'static, CursorIter<IDCursor>> {
        let cursor = egg_mode::user::friends_ids(acct, &self.app_token).with_page_size(5000);

        self.app_limited_client
            .cursor_stream(Method::USER_FOLLOWED_IDS, cursor)
    }

    pub fn follower_ids_self(&self) -> RateLimitedStream<'static, CursorIter<IDCursor>> {
        self.follower_ids(self.user.id)
    }

    pub fn followed_ids_self(&self) -> RateLimitedStream<'static, CursorIter<IDCursor>> {
        self.followed_ids(self.user.id)
    }

    pub fn lookup_users<'a, T, I: IntoIterator<Item = T>>(
        &'a self,
        ids: I,
    ) -> impl Stream<Item = EggModeResult<TwitterUser>> + 'a
    where
        T: Into<UserID> + Unpin + Send,
    {
        let mut futs = vec![];

        let user_ids = ids.into_iter().map(Into::into).collect::<Vec<UserID>>();
        let chunks = user_ids.chunks(100);

        for chunk in chunks {
            futs.push(egg_mode::user::lookup(chunk.to_vec(), &self.app_token).boxed_local());
        }

        self.app_limited_client
            .futures_stream(Method::USER_LOOKUP, futs.into_iter())
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
    ) -> Result<HashMap<u64, bool>> {
        let mut status_map = HashMap::new();

        let chunks = try_join_all(
            ids.into_iter()
                .collect::<Vec<u64>>()
                .chunks(100)
                .map(|chunk| egg_mode::tweet::lookup_map(chunk.to_vec(), &self.app_token)),
        )
        .await?
        .into_iter();

        for chunk in chunks {
            for (k, v) in chunk.response {
                status_map.insert(k, v);
            }
        }

        Ok(status_map.iter().map(|(k, v)| (*k, v.is_some())).collect())
    }

    pub async fn lookup_users_cached(&self, ids: &[u64]) -> Result<Vec<TwitterUser>> {
        let cache = self.user_cache.read().unwrap();

        let unknown_ids = ids
            .iter()
            .cloned()
            .filter(|id| !cache.by_id.contains_key(id))
            .collect::<Vec<u64>>();
        drop(cache);

        let new_users = try_join_all(
            unknown_ids
                .chunks(100)
                .map(|chunk| egg_mode::user::lookup(chunk.to_vec(), &self.token)),
        )
        .await?
        .into_iter()
        .map(|r| r.response)
        .flatten()
        .collect::<Vec<_>>();

        let mut writeable_cache = self.user_cache.write().unwrap();

        for user in new_users {
            writeable_cache
                .id_by_screen_name
                .insert(user.screen_name.clone(), user.id);
            writeable_cache.by_id.insert(user.id, user);
        }

        let mut res = Vec::with_capacity(ids.len());

        for id in ids {
            // The blocks endpoint may return IDs for users that no longer exist, so we ignore empty values here.
            if let Some(user) = writeable_cache.by_id.get(id) {
                res.push(user.clone());
            }
        }

        Ok(res)
    }

    pub fn user_timeline_stream<T: Into<UserID>>(
        &self,
        user: T,
        wait: Duration,
    ) -> Pin<Box<dyn Stream<Item = EggModeResult<Tweet>>>> {
        let timeline = egg_mode::tweet::user_timeline(user, true, true, &self.app_token);

        timeline::TimelineStream::make(timeline, wait)
    }
}

const STATUS_PATTERN: &str = r"^http[s]?://twitter\.com/[^/]+/status/(\d+)(?:\?.+)?$";

pub fn extract_status_id(url: &str) -> Option<u64> {
    Regex::new(STATUS_PATTERN).ok().and_then(|re| {
        re.captures(url)
            .and_then(|groups| groups.get(1).and_then(|m| m.as_str().parse::<u64>().ok()))
    })
}
