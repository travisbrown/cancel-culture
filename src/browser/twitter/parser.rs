use crate::smp::extract_postings;
use chrono::{DateTime, TimeZone, Utc};
use flate2::read::GzDecoder;
use html5ever::driver::{self, ParseOpts};
use html5ever::tendril::TendrilSink;
use lazy_static::lazy_static;
use scraper::element_ref::ElementRef;
use scraper::node::Node;
use scraper::selector::Selector;
use scraper::Html;
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct BrowserTweet {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub time: DateTime<Utc>,
    pub user_id: u64,
    pub user_screen_name: String,
    pub user_name: String,
    pub text: String,
}

impl BrowserTweet {
    pub fn new(
        id: u64,
        parent_id: Option<u64>,
        time: DateTime<Utc>,
        user_id: u64,
        user_screen_name: String,
        user_name: String,
        text: String,
    ) -> BrowserTweet {
        BrowserTweet {
            id,
            parent_id,
            time,
            user_id,
            user_screen_name,
            user_name,
            text,
        }
    }

    fn new_with_timestamp(
        id: u64,
        parent_id: Option<u64>,
        timestamp: i64,
        user_id: u64,
        user_screen_name: String,
        user_name: String,
        text: String,
    ) -> BrowserTweet {
        Self::new(
            id,
            parent_id,
            Utc.timestamp_millis(timestamp),
            user_id,
            user_screen_name,
            user_name,
            text,
        )
    }
}

#[derive(Debug, Deserialize)]
struct TweetUserJson {
    id: u64,
    screen_name: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ExtendedTweetJson {
    full_text: String,
}

#[derive(Debug, Deserialize)]
struct TweetJson {
    id: u64,
    in_reply_to_status_id: Option<u64>,
    timestamp_ms: String,
    user: TweetUserJson,
    text: String,
    extended_tweet: Option<ExtendedTweetJson>,
}

impl TweetJson {
    fn into_browser_tweet(self) -> BrowserTweet {
        BrowserTweet::new_with_timestamp(
            self.id,
            self.in_reply_to_status_id,
            self.timestamp_ms
                .parse::<i64>()
                .expect("Invalid timestamp_ms value"),
            self.user.id,
            self.user.screen_name,
            self.user.name,
            self.extended_tweet
                .map_or(self.text, |extended| extended.full_text),
        )
    }
}

lazy_static! {
    static ref TIME_SEL: Selector = Selector::parse("small.time span._timestamp").unwrap();
    static ref TEXT_SEL: Selector = Selector::parse("p.tweet-text").unwrap();
    static ref TWEET_DIV_SEL: Selector = Selector::parse("div.tweet").unwrap();
    static ref DESCRIPTION_SEL: Selector =
        Selector::parse("meta[property='og:description']").unwrap();
    static ref CANONICAL_SEL: Selector = Selector::parse("link[rel='canonical']").unwrap();
    static ref STATUS_ID_RE: regex::Regex = regex::Regex::new(r"/status/(\d+)$").unwrap();
    static ref PHC_DIV_SEL: Selector = Selector::parse("div.ProfileHeaderCard").unwrap();
    static ref PHC_SCREEN_NAME_SEL: Selector =
        Selector::parse("a.ProfileHeaderCard-screennameLink").unwrap();
    static ref PHC_BIO_SEL: Selector = Selector::parse("p.ProfileHeaderCard-bio").unwrap();
    static ref PHC_LOCATION_SEL: Selector =
        Selector::parse("span.ProfileHeaderCard-locationText").unwrap();
    static ref PHC_URL_SEL: Selector = Selector::parse("span.ProfileHeaderCard-urlText a").unwrap();
    static ref PHC_JOINDATE_SEL: Selector =
        Selector::parse("span.ProfileHeaderCard-joinDateText").unwrap();
    static ref PHC_BIRTHDATE_SEL: Selector =
        Selector::parse("span.ProfileHeaderCard-birthdateText").unwrap();
}

pub fn parse_html<R: Read>(input: &mut R) -> Result<Html, std::io::Error> {
    let parser = driver::parse_document(Html::new_document(), ParseOpts::default()).from_utf8();

    parser.read_from(input)
}

pub fn parse_html_gz<R: Read>(input: &mut R) -> Result<Html, std::io::Error> {
    let mut gz = GzDecoder::new(input);

    parse_html(&mut gz)
}

pub fn extract_description(doc: &Html) -> Option<String> {
    let res = doc
        .select(&DESCRIPTION_SEL)
        .filter_map(|el| el.value().attr("content").map(|value| value.to_string()));

    res.into_iter().next()
}

pub fn extract_canonical_status_id(doc: &Html) -> Option<u64> {
    doc.select(&CANONICAL_SEL)
        .filter_map(|el| {
            el.value().attr("href").and_then(|href| {
                STATUS_ID_RE.captures(href).and_then(|captures| {
                    captures
                        .get(1)
                        .and_then(|capture| capture.as_str().parse::<u64>().ok())
                })
            })
        })
        .next()
}

pub fn extract_tweets(doc: &Html) -> Vec<BrowserTweet> {
    match extract_postings(doc) {
        Ok(Some(postings)) => postings
            .iter()
            .map(|posting| {
                BrowserTweet::new(
                    posting.identifier.parse::<u64>().unwrap(),
                    None,
                    posting.date_created,
                    posting.author.identifier.parse::<u64>().unwrap(),
                    posting.author.additional_name.clone(),
                    posting.author.given_name.clone(),
                    posting.article_body.clone(),
                )
            })
            .collect(),
        _ => doc
            .select(&TWEET_DIV_SEL)
            .filter_map(|el| extract_div_tweet(&el))
            .collect(),
    }
}

pub fn extract_phcs(doc: &Html) -> Vec<(String, String, String, String, String, Option<String>)> {
    doc.select(&PHC_DIV_SEL)
        .filter_map(|el| extract_phc(&el))
        .collect()
}

fn extract_phc(
    element_ref: &ElementRef,
) -> Option<(String, String, String, String, String, Option<String>)> {
    let screen_name = element_ref
        .select(&PHC_SCREEN_NAME_SEL)
        .next()
        .and_then(|el| el.value().attr("href").map(|v| v.split_at(1).1.to_string()))?;
    let bio = element_ref
        .select(&PHC_BIO_SEL)
        .next()
        .map(|el| el.inner_html().trim().to_string())?;
    let location = element_ref
        .select(&PHC_LOCATION_SEL)
        .next()
        .map(|el| el.inner_html().trim().to_string())?;
    let url = element_ref
        .select(&PHC_URL_SEL)
        .next()
        .and_then(|el| el.value().attr("title"))?;
    let join_date = element_ref
        .select(&PHC_JOINDATE_SEL)
        .next()
        .and_then(|el| el.value().attr("title"))?;
    let birth_date = element_ref
        .select(&PHC_BIRTHDATE_SEL)
        .next()
        .and_then(|el| el.value().attr("title").map(|v| v.to_string()));

    Some((
        screen_name,
        bio,
        location,
        url.to_string(),
        join_date.to_string(),
        birth_date,
    ))
}

pub fn extract_tweet_json(content: &str) -> Option<BrowserTweet> {
    let t: serde_json::Result<TweetJson> = serde_json::from_str(content);
    t.ok().map(|v| v.into_browser_tweet())
}

fn extract_div_tweet(element_ref: &ElementRef) -> Option<BrowserTweet> {
    let element = element_ref.value();

    let id = element
        .attr("data-tweet-id")
        .and_then(|v| v.parse::<u64>().ok());
    let parent_id = element
        .attr("data-conversation-id")
        .and_then(|v| v.parse::<u64>().ok());
    let user_id = element
        .attr("data-user-id")
        .and_then(|v| v.parse::<u64>().ok());
    let user_screen_name = element.attr("data-screen-name");
    let user_name = element.attr("data-name");
    let timestamp = element_ref.select(&TIME_SEL).next().and_then(|el| {
        el.value()
            .attr("data-time-ms")
            .and_then(|v| v.parse::<i64>().ok())
    });
    let text = element_ref.select(&TEXT_SEL).next().map(|el| {
        let mut result = String::new();

        for child_ref in el.children() {
            match child_ref.value() {
                Node::Text(text) => {
                    result.push_str(&text.text);
                }
                Node::Element(child_el) => {
                    if child_el.name() == "img" {
                        if let Some(alt) = child_el.attr("alt") {
                            result.push_str(alt);
                        }
                    } else if child_el.name() == "a" {
                        let text = ElementRef::wrap(child_ref)
                            .unwrap()
                            .text()
                            .map(|t| t.trim())
                            // Previously we excluded pic links here with
                            // `&& !v.starts_with("pic.twitter.com")`
                            .filter(|v| !v.is_empty())
                            .collect::<Vec<_>>()
                            .join("");

                        if !text.starts_with("http") {
                            result.push(' ');
                            result.push_str(&text);
                            result.push(' ');
                        } else if let Some(url) = child_el.attr("data-expanded-url") {
                            result.push_str(&format!(" {} ", url));
                        }
                    }
                }
                _ => (),
            }
        }

        result.trim().to_string()
    });

    id.zip(user_id)
        .zip(Some(parent_id.unwrap_or(0)))
        .zip(user_screen_name)
        .zip(user_name)
        .zip(timestamp)
        .zip(text)
        .map(
            |((((((id, user_id), parent_id), user_screen_name), user_name), timestamp), text)| {
                BrowserTweet::new_with_timestamp(
                    id,
                    if parent_id == id {
                        None
                    } else {
                        Some(parent_id)
                    },
                    timestamp,
                    user_id,
                    user_screen_name.trim().to_string(),
                    user_name.trim().to_string(),
                    text,
                )
            },
        )
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use flate2::read::GzDecoder;
    use scraper::Html;
    use std::fs::{read_to_string, File};
    use std::io::Read;

    #[test]
    fn extract_canonical_status_id() {
        let file = File::open("examples/wayback/53SGIJNJMTP6S626CVRCHFTX3OEWXB3E.gz").unwrap();
        let mut gz = GzDecoder::new(file);
        let mut html = String::new();

        gz.read_to_string(&mut html).unwrap();

        let doc = Html::parse_document(&html);

        assert_eq!(
            super::extract_canonical_status_id(&doc),
            Some(1170761943067631621)
        );
    }

    #[test]
    fn extract_tweets() {
        let file = File::open("examples/wayback/53SGIJNJMTP6S626CVRCHFTX3OEWXB3E.gz").unwrap();
        let mut gz = GzDecoder::new(file);
        let mut html = String::new();

        gz.read_to_string(&mut html).unwrap();

        let doc = Html::parse_document(&html);

        assert_eq!(
            super::extract_description(&doc).map(|value| value.len()),
            Some(293)
        );
        assert_eq!(super::extract_tweets(&doc).len(), 11);
    }

    #[test]
    fn extract_tweets_json() {
        let contents = read_to_string("examples/json/890659426796945408.json").unwrap();
        let expected = super::BrowserTweet::new(
            890659426796945408,
            None,
            Utc.timestamp_millis(1501184729657),
            849768899772133376,
            "DrupalLeaks".to_string(),
            "DrupalLeaks".to_string(),
            "Whose secrets should we cover in Part 2 of our documentary series, \
                   The Dark Secrets of Drupal? Or perhaps some other #DrupalElite? \
                   Speak up!"
                .to_string(),
        );

        assert_eq!(super::extract_tweet_json(&contents), Some(expected));
    }
}
