use crate::twitter::store::wayback;
use fantoccini::error::CmdError;
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use thiserror::Error;
use tokio::task::JoinError;

#[derive(Error, Debug)]
pub enum Error {
    ClientError(#[from] reqwest::Error),
    ItemError(#[from] wayback_rs::item::Error),
    ItemParsingError(String),
    ItemDecodingError(#[from] serde_json::Error),
    FileIOError(#[from] std::io::Error),
    StoreContentsDecodingError(#[from] csv::Error),
    StoreContentsEncodingError(#[from] csv::IntoInnerError<csv::Writer<Vec<u8>>>),
    BrowserError(#[from] CmdError),
    TaskError(#[from] JoinError),
    TweetStoreError(#[from] wayback::TweetStoreError),
    DataPathError(PathBuf),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        Debug::fmt(self, f)
    }
}

/*
impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::ClientError(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::ItemDecodingError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::FileIOError(e)
    }
}

impl From<csv::Error> for Error {
    fn from(e: csv::Error) -> Self {
        Error::StoreContentsDecodingError(e)
    }
}

impl From<csv::IntoInnerError<csv::Writer<Vec<u8>>>> for Error {
    fn from(e: csv::IntoInnerError<csv::Writer<Vec<u8>>>) -> Self {
        Error::StoreContentsEncodingError(Box::new(e))
    }
}

impl From<CmdError> for Error {
    fn from(e: CmdError) -> Self {
        Error::BrowserError(e)
    }
}

impl From<JoinError> for Error {
    fn from(e: JoinError) -> Self {
        Error::TaskError(e)
    }
}

impl From<wayback::TweetStoreError> for Error {
    fn from(e: wayback::TweetStoreError) -> Self {
        Error::TweetStoreError(e)
    }
}
*/
