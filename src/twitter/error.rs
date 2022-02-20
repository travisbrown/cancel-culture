use displaydoc::Display;
use thiserror::Error;

use std::fmt;
use std::path::PathBuf;

#[derive(Error, Display)]
pub enum Error {
    /// failed to parse config file contents: {0}
    ConfigParseError(#[from] toml::de::Error),
    /// failed to read config file from {1}: {0}
    ConfigReadError(#[source] std::io::Error, PathBuf),
    /// failure to read from stdin: {0}
    StdinError(#[source] std::io::Error),
    /// a twitter API call failed: {0}
    ApiError(#[from] egg_mode::error::Error),
    /// an failure occurred when operating the headless browser: {0}
    BrowserError(#[from] fantoccini::error::CmdError),
    /// an error occurred in the http client: {0}
    HttpClientError(#[from] reqwest::Error),
    /// an error occurred when piloting the wayback machine subcrate: {0}
    WaybackClientError(#[from] crate::wayback::Error),
    /// failure to read from CDX JSON file: {0}
    CdxJsonError(#[source] std::io::Error),
    /// a failure occurred when parsing a tweet id string: {0}
    TweetIDParseError(String),
    /// the tweet ID {0}, which was supposed to be a reply, was not a reply
    NotReplyError(u64),
    /// the user with ID {0} was not found
    MissingUserError(u64),
    /// the provided token could not be used: {0:?}
    UnsupportedTokenMethod(super::Method),
    /// egg-mode-extras error (temporary workaround during migration)
    EggModeExtras(#[from] egg_mode_extras::error::Error),
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
