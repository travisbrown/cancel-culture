use egg_mode::KeyPair;
use serde_derive::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    twitter: TwitterConfig,
}

impl Config {
    pub fn twitter_key_pairs(&self) -> (KeyPair, KeyPair) {
        self.twitter.key_pairs()
    }
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
    fn key_pairs(&self) -> (KeyPair, KeyPair) {
        (
            KeyPair::new(self.consumer_key.clone(), self.consumer_secret.clone()),
            KeyPair::new(self.access_token.clone(), self.access_token_secret.clone()),
        )
    }
}
