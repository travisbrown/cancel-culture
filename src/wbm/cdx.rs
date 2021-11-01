use reqwest::Client as RClient;
use std::io::{BufReader, Read};
use std::time::Duration;
use thiserror::Error;
use wayback_rs::Item;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Item parsing error: {0}")]
    ItemParsingError(#[from] wayback_rs::item::Error),
    #[error("HTTP client error: {0}")]
    HttpClientError(#[from] reqwest::Error),
    #[error("JSON decoding error: {0}")]
    JsonError(#[from] serde_json::Error),
}

pub struct Client {
    underlying: RClient,
}

impl Client {
    const CDX_BASE: &'static str = "http://web.archive.org/cdx/search/cdx";
    const CDX_OPTIONS: &'static str =
        "&output=json&fl=original,timestamp,digest,mimetype,statuscode";

    pub fn new() -> Client {
        Client {
            underlying: RClient::builder()
                .tcp_keepalive(Some(Duration::from_secs(20)))
                .build()
                .unwrap(),
        }
    }

    fn decode_rows(rows: Vec<Vec<String>>) -> Result<Vec<Item>, Error> {
        rows.into_iter()
            .skip(1)
            .map(|row| {
                Item::parse_optional_record(
                    row.get(0).map(|v| v.as_str()),
                    row.get(1).map(|v| v.as_str()),
                    row.get(2).map(|v| v.as_str()),
                    row.get(3).map(|v| v.as_str()),
                    Some("0"),
                    row.get(4).map(|v| v.as_str()),
                )
                .map_err(From::from)
            })
            .collect()
    }

    pub fn load_json<R: Read>(reader: R) -> Result<Vec<Item>, Error> {
        let buffered = BufReader::new(reader);

        let rows = serde_json::from_reader::<BufReader<R>, Vec<Vec<String>>>(buffered)?;

        Self::decode_rows(rows)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Item>, Error> {
        let query_url = format!("{}?url={}{}", Client::CDX_BASE, query, Client::CDX_OPTIONS);
        let rows = self
            .underlying
            .get(&query_url)
            .send()
            .await?
            .json::<Vec<Vec<String>>>()
            .await?;

        Self::decode_rows(rows)
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Client;
    use std::fs::File;

    #[test]
    fn test_client_decode_rows() {
        let file = File::open("examples/wayback/cdx-result.json").unwrap();
        let result = Client::load_json(file).unwrap();

        assert_eq!(result.len(), 37);
    }
}
