pub mod config;
mod error;
mod method;
mod method_limit;
mod rate_limited;
pub mod store;
pub mod timeline;

pub use error::Error;
pub use method::Method;
pub use method_limit::{MethodLimit, MethodLimitStore};
use rate_limited::{RateLimitedClient, TimelineScrollback};

use egg_mode::{
    tweet::Tweet,
    user::{TwitterUser, UserID},
    KeyPair, Token,
};
use futures::{future::try_join_all, stream::LocalBoxStream, FutureExt, TryStreamExt};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub type Result<T> = std::result::Result<T, Error>;
pub type EggModeResult<T> = std::result::Result<T, egg_mode::error::Error>;

const TWEET_LOOKUP_PAGE_SIZE: usize = 100;
const USER_FOLLOWER_IDS_PAGE_SIZE: i32 = 5000;
const USER_FOLLOWED_IDS_PAGE_SIZE: i32 = 5000;
const USER_LOOKUP_PAGE_SIZE: usize = 100;
const USER_TIMELINE_PAGE_SIZE: i32 = 200;

pub struct Client {
    token: Token,
    app_token: Token,
    user: TwitterUser,
    app_limited_client: RateLimitedClient,
}

impl Client {
    async fn new(
        token: Token,
        app_token: Token,
        user: TwitterUser,
    ) -> egg_mode::error::Result<Client> {
        let app_limited_client = RateLimitedClient::new(app_token.clone()).await?;

        Ok(Client {
            token,
            app_token,
            user,
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
    ) -> LocalBoxStream<EggModeResult<Tweet>> {
        self.app_limited_client.make_stream(
            TimelineScrollback::new(
                egg_mode::tweet::user_timeline(acct, with_replies, with_rts, &self.app_token)
                    .with_page_size(USER_TIMELINE_PAGE_SIZE),
            ),
            Method::USER_TIMELINE,
        )
    }

    pub fn follower_ids<T: Into<UserID>>(&self, acct: T) -> LocalBoxStream<EggModeResult<u64>> {
        let cursor = egg_mode::user::followers_ids(acct, &self.app_token)
            .with_page_size(USER_FOLLOWER_IDS_PAGE_SIZE);

        self.app_limited_client
            .make_stream(cursor, Method::USER_FOLLOWER_IDS)
    }

    pub fn followed_ids<T: Into<UserID>>(&self, acct: T) -> LocalBoxStream<EggModeResult<u64>> {
        let cursor = egg_mode::user::friends_ids(acct, &self.app_token)
            .with_page_size(USER_FOLLOWED_IDS_PAGE_SIZE);

        self.app_limited_client
            .make_stream(cursor, Method::USER_FOLLOWED_IDS)
    }

    pub fn follower_ids_self(&self) -> LocalBoxStream<EggModeResult<u64>> {
        self.follower_ids(self.user.id)
    }

    pub fn followed_ids_self(&self) -> LocalBoxStream<EggModeResult<u64>> {
        self.followed_ids(self.user.id)
    }

    pub fn lookup_users<'a, T, I: IntoIterator<Item = T>>(
        &'a self,
        ids: I,
    ) -> LocalBoxStream<'a, EggModeResult<TwitterUser>>
    where
        T: Into<UserID> + Unpin + Send,
    {
        let mut futs = vec![];

        let user_ids = ids.into_iter().map(Into::into).collect::<Vec<UserID>>();
        let chunks = user_ids.chunks(USER_LOOKUP_PAGE_SIZE);

        for chunk in chunks {
            futs.push(egg_mode::user::lookup(chunk.to_vec(), &self.app_token).boxed_local());
        }

        let iter = futs.into_iter();

        self.app_limited_client
            .make_stream(iter.peekable(), Method::USER_LOOKUP)
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
                .chunks(TWEET_LOOKUP_PAGE_SIZE)
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

    pub fn user_timeline_stream<T: Into<UserID>>(
        &self,
        user: T,
        wait: Duration,
    ) -> LocalBoxStream<EggModeResult<Tweet>> {
        let timeline = egg_mode::tweet::user_timeline(user, true, true, &self.app_token);

        timeline::make_stream(timeline, wait)
    }
}

const STATUS_PATTERN: &str = r"^http[s]?://twitter\.com/[^/]+/status/(\d+)(?:\?.+)?$";

pub fn extract_status_id(url: &str) -> Option<u64> {
    Regex::new(STATUS_PATTERN).ok().and_then(|re| {
        re.captures(url)
            .and_then(|groups| groups.get(1).and_then(|m| m.as_str().parse::<u64>().ok()))
    })
}
