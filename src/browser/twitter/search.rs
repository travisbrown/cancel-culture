use super::super::Scroller;
use chrono::{format::ParseError, NaiveDate};
use fantoccini::{error::CmdError, Client, Locator};
use futures::future::BoxFuture;
use regex::Regex;

pub struct UserTweetSearch(String, NaiveDate, NaiveDate, String, Regex);

impl UserTweetSearch {
    const TWEET_LINK_LOC: Locator<'static> = Locator::XPath("//a[contains(@href, '/status/')]");
    const SEARCH_DATE_FMT: &'static str = "%Y-%m-%d";

    pub fn parse(screen_name: &str, from: &str, to: &str) -> Result<Self, ParseError> {
        let from_date = NaiveDate::parse_from_str(&from, Self::SEARCH_DATE_FMT)?;
        let to_date = NaiveDate::parse_from_str(&to, Self::SEARCH_DATE_FMT)?;
        let pattern = format!("{}/status/\\d+$", screen_name);

        Ok(UserTweetSearch(
            screen_name.to_string(),
            from_date,
            to_date,
            Self::make_url(screen_name, &from_date, &to_date),
            Regex::new(&pattern).unwrap(),
        ))
    }

    fn make_url(screen_name: &str, from: &NaiveDate, to: &NaiveDate) -> String {
        format!(
            "https://twitter.com/search?q=(from%3A{})%20until%3A{}%20since%3A{}&src=typed_query",
            screen_name,
            to.format(Self::SEARCH_DATE_FMT),
            from.format(Self::SEARCH_DATE_FMT)
        )
    }
}

impl Scroller for UserTweetSearch {
    type Item = String;
    type Err = CmdError;

    fn init<'a>(&'a self, client: &'a mut Client) -> BoxFuture<'a, Result<(), Self::Err>> {
        Box::pin(client.goto(&self.3))
    }

    fn extract<'a>(
        &'a self,
        client: &'a mut Client,
    ) -> BoxFuture<'a, Result<Vec<Self::Item>, Self::Err>> {
        Box::pin(async move {
            let elements = client.find_all(Self::TWEET_LINK_LOC).await?;

            let mut urls = Vec::with_capacity(elements.len());

            for mut element in elements {
                if let Some(url) = element.attr("href").await? {
                    if self.4.is_match(&url) {
                        urls.push(url);
                    }
                }
            }

            Ok(urls)
        })
    }
}
