use chrono::{DateTime, TimeZone, Utc};
use flate2::read::GzDecoder;
use lazy_static::lazy_static;
use scraper::element_ref::ElementRef;
use scraper::node::Node;
use scraper::selector::Selector;
use scraper::Html;
use std::io::Read;

#[derive(Debug)]
pub struct BrowserTweet {
    pub id: u64,
    pub time: DateTime<Utc>,
    user_id: u64,
    pub user_screen_name: String,
    pub text: String,
}

lazy_static! {
    static ref TIME_SEL: Selector = Selector::parse("small.time span._timestamp").unwrap();
    static ref TEXT_SEL: Selector = Selector::parse("p.tweet-text").unwrap();
    static ref TWEET_DIV_SEL: Selector = Selector::parse("div.tweet").unwrap();
    static ref DESCRIPTION_SEL: Selector =
        Selector::parse("meta[property='og:description']").unwrap();
}

pub fn parse<R: Read>(input: &mut R) -> Result<Html, std::io::Error> {
    let mut html = String::new();

    input.read_to_string(&mut html)?;

    Ok(Html::parse_document(&html))
}

pub fn parse_gz<R: Read>(input: &mut R) -> Result<Html, std::io::Error> {
    let mut gz = GzDecoder::new(input);
    let mut html = String::new();

    gz.read_to_string(&mut html)?;

    Ok(Html::parse_document(&html))
}

pub fn extract_description(doc: &Html) -> Option<String> {
    let res = doc
        .select(&DESCRIPTION_SEL)
        .filter_map(|el| el.value().attr("content").map(|value| value.to_string()));

    res.into_iter().next()
}

pub fn extract_tweets(doc: &Html) -> Vec<BrowserTweet> {
    doc.select(&TWEET_DIV_SEL)
        .filter_map(|el| extract_div_tweet(&el))
        .collect()
}

fn extract_div_tweet(element_ref: &ElementRef) -> Option<BrowserTweet> {
    let element = element_ref.value();

    let id = element
        .attr("data-tweet-id")
        .and_then(|v| v.parse::<u64>().ok());
    let user_id = element
        .attr("data-user-id")
        .and_then(|v| v.parse::<u64>().ok());
    let user_screen_name = element.attr("data-screen-name");
    let timestamp = element_ref.select(&TIME_SEL).next().and_then(|el| {
        el.value()
            .attr("data-time")
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
                    }
                }
                _ => (),
            }
        }

        result
    });

    id.zip(user_id)
        .zip(user_screen_name)
        .zip(timestamp)
        .zip(text)
        .map(
            |((((id, user_id), user_screen_name), timestamp), text)| BrowserTweet {
                id,
                time: Utc.timestamp(timestamp, 0),
                user_id,
                user_screen_name: user_screen_name.to_string(),
                text: text.to_string(),
            },
        )
}

#[cfg(test)]
mod tests {
    use flate2::read::GzDecoder;
    use scraper::Html;
    use std::fs::File;
    use std::io::Read;

    #[test]
    fn extract() {
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
}
