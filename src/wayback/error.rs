use fantoccini::error::CmdError;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug)]
pub enum Error {
    ClientError(reqwest::Error),
    ItemParsingError(String),
    ItemDecodingError(serde_json::Error),
    FileIOError(std::io::Error),
    StoreContentsDecodingError(csv::Error),
    StoreContentsEncodingError(Box<csv::IntoInnerError<csv::Writer<Vec<u8>>>>),
    BrowserError(CmdError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        Debug::fmt(self, f)
    }
}

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
