use super::super::Scroller;
use chrono::{format::ParseError, NaiveDate};
use fantoccini::{Client, Locator};
use futures::{future::BoxFuture, FutureExt};
use regex::Regex;
use std::time::Duration;
use tokio::time::sleep;

pub struct UserTweetSearch {
    screen_name: String,
    from: NaiveDate,
    to: NaiveDate,
    search_url: String,
    pattern: Regex,
}

impl UserTweetSearch {
    const TWEET_LINK_LOC: Locator<'static> = Locator::XPath("//a[contains(@href, '/status/')]");
    const NO_RESULTS_LOC: Locator<'static> =
        Locator::XPath("//span[contains(text(), 'No results for ')]");
    const RATE_LIMIT_LOC: Locator<'static> =
        Locator::XPath("//span[contains(text(), 'Something went wrong')]");
    const SEARCH_DATE_FMT: &'static str = "%Y-%m-%d";
    const SPLIT_CUTOFF: usize = 40;
    const BROWSER_RATE_LIMIT_WAIT_SECONDS: u64 = 250;

    pub fn new(screen_name: &str, from: &NaiveDate, to: &NaiveDate) -> Self {
        let pattern = format!("{}/status/(\\d+)$", screen_name);

        UserTweetSearch {
            screen_name: screen_name.to_string(),
            from: *from,
            to: *to,
            search_url: Self::make_url(screen_name, from, to),
            pattern: Regex::new(&pattern).unwrap(),
        }
    }

    pub fn parse(screen_name: &str, from: &str, to: &str) -> Result<Self, ParseError> {
        let from_date = NaiveDate::parse_from_str(from, Self::SEARCH_DATE_FMT)?;
        let to_date = NaiveDate::parse_from_str(to, Self::SEARCH_DATE_FMT)?;

        Ok(Self::new(screen_name, &from_date, &to_date))
    }

    fn make_url(screen_name: &str, from: &NaiveDate, to: &NaiveDate) -> String {
        format!(
            "https://twitter.com/search?q=(from%3A{})%20until%3A{}%20since%3A{}&src=typed_query",
            screen_name,
            to.format(Self::SEARCH_DATE_FMT),
            from.format(Self::SEARCH_DATE_FMT)
        )
    }

    pub async fn extract_all_split(&self, client: &mut Client) -> Result<Vec<u64>, anyhow::Error> {
        let all = self.extract_all(client).await?;
        let len = (self.to - self.from).num_days();

        if all.len() >= Self::SPLIT_CUTOFF && len > 1 {
            let mut result = Vec::with_capacity(all.len());

            for day in self.from.iter_days().take_while(|day| *day < self.to) {
                result.extend(
                    Self::new(&self.screen_name, &day, &day.succ())
                        .extract_all(client)
                        .await?,
                );
            }

            Ok(result)
        } else {
            Ok(all)
        }
    }
}

impl Scroller for UserTweetSearch {
    type Item = u64;
    type Err = anyhow::Error;

    fn init<'a>(&'a self, client: &'a mut Client) -> BoxFuture<'a, Result<bool, Self::Err>> {
        async move {
            client.goto(&self.search_url).await?;
            sleep(Duration::from_millis(750)).await;
            log::info!("Checking: {}", self.search_url);

            if client.find_all(Self::NO_RESULTS_LOC).await?.is_empty() {
                if !client.find_all(Self::RATE_LIMIT_LOC).await?.is_empty() {
                    sleep(Duration::from_secs(Self::BROWSER_RATE_LIMIT_WAIT_SECONDS)).await;

                    self.init(client).await
                } else {
                    Ok(true)
                }
            } else {
                Ok(false)
            }
        }
        .boxed()
    }

    fn extract<'a>(
        &'a self,
        client: &'a mut Client,
    ) -> BoxFuture<'a, Result<Vec<Self::Item>, Self::Err>> {
        async move {
            let elements = client.find_all(Self::TWEET_LINK_LOC).await?;

            let mut ids = Vec::with_capacity(elements.len());

            for element in elements {
                if let Ok(Some(url)) = element.attr("href").await {
                    if let Some(caps) = self.pattern.captures(&url) {
                        if let Ok(id) = caps[1].parse::<u64>() {
                            ids.push(id);
                        }
                    }
                }
            }

            Ok(ids)
        }
        .boxed()
    }
}
