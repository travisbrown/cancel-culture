use lazy_static::lazy_static;
use regex::Regex;

const TWEET_URL_PATTERN: &str = r"^http[s]?://twitter\.com/([^/]+)/status/(\d+)(?:\?.+)?$";
const TWEET_REDIRECT_HTML_PATTERN: &str = r#"^<html><body>You are being <a href="https://twitter.com/([^/]+)/status/(\d+)(?:\?.+)?">redirected</a>.</body></html>$"#;

pub fn parse_tweet_url(url: &str) -> Option<(String, u64)> {
    lazy_static! {
        static ref TWEET_URL_RE: Regex = Regex::new(TWEET_URL_PATTERN).unwrap();
    }

    TWEET_URL_RE.captures(url).and_then(|groups| {
        groups
            .get(1)
            .map(|m| m.as_str().to_string())
            .zip(groups.get(2).and_then(|m| m.as_str().parse::<u64>().ok()))
    })
}

pub fn parse_tweet_redirect_html(content: &str) -> Option<(String, u64)> {
    lazy_static! {
        static ref TWEET_REDIRECT_HTML_RE: Regex = Regex::new(TWEET_REDIRECT_HTML_PATTERN).unwrap();
    }

    TWEET_REDIRECT_HTML_RE.captures(content).and_then(|groups| {
        groups
            .get(1)
            .map(|m| m.as_str().to_string())
            .zip(groups.get(2).and_then(|m| m.as_str().parse::<u64>().ok()))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_tweet_url() {
        let pairs = vec![
            (
                "https://twitter.com/martinshkreli/status/446273988780904448?lang=da",
                Some(("martinshkreli".to_string(), 446273988780904448)),
            ),
            (
                "https://twitter.com/ChiefScientist/status/1270099974559154177",
                Some(("ChiefScientist".to_string(), 1270099974559154177)),
            ),
            ("abcdef", None),
        ];

        for (url, expected) in pairs {
            assert_eq!(super::parse_tweet_url(url), expected);
        }
    }

    #[test]
    fn test_parse_tweet_redirect_html() {
        let content = r#"<html><body>You are being <a href="https://twitter.com/brithume/status/1283385533415206914">redirected</a>.</body></html>"#;

        assert_eq!(
            super::parse_tweet_redirect_html(content),
            Some(("brithume".to_string(), 1283385533415206914))
        );
    }
}
