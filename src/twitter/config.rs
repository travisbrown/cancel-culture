use egg_mode::KeyPair;
use serde_derive::Deserialize;

#[derive(Debug, Deserialize, Eq, PartialEq)]
pub struct Config {
    twitter: TwitterConfig,
}

impl Config {
    pub fn twitter_key_pairs(&self) -> (KeyPair, KeyPair) {
        self.twitter.key_pairs()
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TwitterConfig {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_token_secret: String,
}

impl TwitterConfig {
    fn key_pairs(&self) -> (KeyPair, KeyPair) {
        (
            KeyPair::new(self.consumer_key.clone(), self.consumer_secret.clone()),
            KeyPair::new(self.access_token.clone(), self.access_token_secret.clone()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_twitter_config() {
        let input =
            "[twitter]\nconsumerKey=\"ABC\"\nconsumerSecret=\"DEF\"\naccessToken=\"GHI\"\naccessTokenSecret=\"JKL\"";

        let config = toml::from_str::<Config>(&input).unwrap();
        let expected = Config {
            twitter: TwitterConfig {
                consumer_key: "ABC".to_string(),
                consumer_secret: "DEF".to_string(),
                access_token: "GHI".to_string(),
                access_token_secret: "JKL".to_string(),
            },
        };

        assert_eq!(config, expected);
    }
}
