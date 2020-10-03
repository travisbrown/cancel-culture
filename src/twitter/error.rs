use std::fmt::{Debug, Display, Formatter};

#[derive(Debug)]
pub enum Error {
    ConfigParseError(toml::de::Error),
    ConfigReadError(std::io::Error),
    ApiError(egg_mode::error::Error),
    BrowserError(fantoccini::error::CmdError),
    HttpClientError(reqwest::Error),
    TweetIDParseError(String),
    NotReplyError(u64),
    MissingUserError(u64),
    UnsupportedTokenMethod(super::Method),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        Debug::fmt(self, f)
    }
}

impl std::error::Error for Error {}

impl From<toml::de::Error> for Error {
    fn from(e: toml::de::Error) -> Self {
        Error::ConfigParseError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::ConfigReadError(e)
    }
}

impl From<egg_mode::error::Error> for Error {
    fn from(e: egg_mode::error::Error) -> Self {
        Error::ApiError(e)
    }
}

impl From<fantoccini::error::CmdError> for Error {
    fn from(e: fantoccini::error::CmdError) -> Self {
        Error::BrowserError(e)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::HttpClientError(e)
    }
}
