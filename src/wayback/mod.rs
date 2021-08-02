pub mod cdx;
mod error;
mod store;
pub mod web;

pub use error::Error;
pub use store::Store;

use chrono::NaiveDateTime;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub url: String,
    pub archived: NaiveDateTime,
    pub digest: String,
    pub mimetype: String,
    pub status: Option<u16>,
}

impl Item {
    const DATE_FMT: &'static str = "%Y%m%d%H%M%S";

    pub fn new(
        url: String,
        archived: NaiveDateTime,
        digest: String,
        mimetype: String,
        status: Option<u16>,
    ) -> Item {
        Item {
            url,
            archived,
            digest,
            mimetype,
            status,
        }
    }

    pub fn wayback_url(&self, original: bool) -> String {
        format!(
            "http://web.archive.org/web/{}{}/{}",
            self.timestamp(),
            if original { "id_" } else { "if_" },
            self.url
        )
    }

    pub fn timestamp(&self) -> String {
        self.archived.format(Item::DATE_FMT).to_string()
    }

    pub fn status_code(&self) -> String {
        self.status.map_or("-".to_string(), |v| v.to_string())
    }

    pub fn infer_extension(&self) -> Option<String> {
        match self.mimetype.as_str() {
            "application/json" => Some("json".to_string()),
            "text/html" => Some("html".to_string()),
            _ => None,
        }
    }

    pub fn infer_filename(&self) -> String {
        self.infer_extension().map_or_else(
            || self.digest.clone(),
            |ext| format!("{}.{}", self.digest, ext),
        )
    }

    fn parse(
        url: &str,
        timestamp: &str,
        digest: &str,
        mimetype: &str,
        status: &str,
    ) -> Result<Item> {
        let archived = NaiveDateTime::parse_from_str(timestamp, Item::DATE_FMT)
            .map_err(|_| Error::ItemParsingError(format!("Unexpected timestamp: {}", timestamp)))?;

        let status_parsed = if status == "-" {
            Ok(None)
        } else {
            status
                .parse::<u16>()
                .map(Some)
                .map_err(|_| Error::ItemParsingError(format!("Unexpected status: {}", status)))
        }?;

        Ok(Item::new(
            url.to_string(),
            archived,
            digest.to_string(),
            mimetype.to_string(),
            status_parsed,
        ))
    }

    fn parse_optional(
        url: Option<&str>,
        timestamp: Option<&str>,
        digest: Option<&str>,
        mimetype: Option<&str>,
        status: Option<&str>,
    ) -> Result<Item> {
        Self::parse(
            url.ok_or_else(|| Error::ItemParsingError("Missing URL".to_string()))?,
            timestamp.ok_or_else(|| Error::ItemParsingError("Missing timestamp".to_string()))?,
            digest.ok_or_else(|| Error::ItemParsingError("Missing digest".to_string()))?,
            mimetype.ok_or_else(|| Error::ItemParsingError("Missing mimetype".to_string()))?,
            status.ok_or_else(|| Error::ItemParsingError("Missing status".to_string()))?,
        )
    }

    fn from_row(row: &[String]) -> Result<Item> {
        if row.len() != 5 {
            Err(Error::ItemParsingError(format!(
                "Invalid item fields: {:?}",
                row
            )))
        } else {
            Item::parse(&row[0], &row[1], &row[2], &row[3], &row[4])
        }
    }
}
