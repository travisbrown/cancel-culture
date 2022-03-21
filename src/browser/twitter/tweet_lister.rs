use crate::browser::twitter::search::UserTweetSearch;
use chrono::{NaiveDate, Utc};
use egg_mode::{tweet::Tweet, user::UserID};
use egg_mode_extras::{client::TokenType, Client};
use fantoccini::Client as FClient;
use futures::TryStreamExt;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub struct TweetLister<'a> {
    client: &'a Client,
    browser: &'a mut FClient,
}

impl<'a> TweetLister<'a> {
    const DAY_CHUNK_SIZE: usize = 20;

    pub fn new(client: &'a Client, browser: &'a mut FClient) -> TweetLister<'a> {
        TweetLister { client, browser }
    }

    fn days(from: NaiveDate, to: NaiveDate) -> Box<dyn Iterator<Item = NaiveDate>> {
        if from < to {
            Box::new(itertools::unfold(from, move |state| {
                *state = state.succ();
                if *state <= to {
                    Some(*state)
                } else {
                    None
                }
            }))
        } else {
            Box::new(itertools::unfold(from, move |state| {
                *state = state.pred();
                if *state >= to {
                    Some(*state)
                } else {
                    None
                }
            }))
        }
    }

    pub async fn get_all<T: Into<UserID> + Clone>(
        &mut self,
        id: T,
    ) -> anyhow::Result<(Vec<u64>, i32)> {
        let user = self.client.lookup_user(id.clone(), TokenType::App).await?;
        let screen_name = user.screen_name;
        let user_created = user.created_at.date().naive_utc();
        let tweet_count = user.statuses_count;

        let from_api = self
            .client
            .user_tweets(id, true, true, TokenType::App)
            .try_collect::<Vec<Tweet>>()
            .await?;

        let end = from_api
            .iter()
            .map(|tweet| tweet.created_at)
            .min()
            .unwrap_or_else(Utc::now)
            .date()
            .naive_utc();

        let mut results = from_api
            .iter()
            .map(|tweet| Reverse(tweet.id))
            .collect::<BinaryHeap<Reverse<u64>>>();

        let days = Self::days(end, user_created).collect::<Vec<_>>();

        for chunk in days.chunks(Self::DAY_CHUNK_SIZE) {
            let ids = UserTweetSearch::new(
                &screen_name,
                chunk.last().expect("Chunk is non-empty"),
                &chunk[0].succ(),
            )
            .extract_all_split(self.browser)
            .await?;

            for id in ids {
                results.push(Reverse(id));
            }
        }

        Ok((
            results.iter().map(|rev| rev.0).collect::<Vec<u64>>(),
            tweet_count,
        ))
    }
}
