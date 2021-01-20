use super::{Item, Result, Store};
use bytes::Bytes;
use futures::{Future, FutureExt, StreamExt, TryStreamExt};
use log::info;
use reqwest::Client as RClient;
use std::io::{BufReader, Read};
use std::time::Duration;

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

    fn decode_rows(rows: Vec<Vec<String>>) -> Result<Vec<Item>> {
        rows.into_iter()
            .skip(1)
            .map(|row| Item::from_row(&row))
            .collect()
    }

    pub fn load_json<R: Read>(reader: R) -> Result<Vec<Item>> {
        let buffered = BufReader::new(reader);

        let rows = serde_json::from_reader::<BufReader<R>, Vec<Vec<String>>>(buffered)?;

        Self::decode_rows(rows)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Item>> {
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

    pub async fn download(&self, item: &Item, original: bool) -> Result<Bytes> {
        let item_url = format!(
            "http://web.archive.org/web/{}{}/{}",
            item.timestamp(),
            if original { "id_" } else { "if_" },
            item.url
        );
        Ok(self.underlying.get(&item_url).send().await?.bytes().await?)
    }

    pub fn save_all<'a>(
        &'a self,
        store: &'a Store,
        items: &'a [Item],
        limit: usize,
    ) -> impl Future<Output = Result<()>> + 'a {
        futures::stream::iter(items)
            .filter(move |item| store.contains(&item).map(|v| !v))
            .map(Ok)
            .try_for_each_concurrent(limit, move |item| {
                info!("Downloading {}", item.url);
                self.download(item, true)
                    .then(move |bytes_result| match bytes_result {
                        Ok(bytes) => store.add(item, bytes).boxed_local(),
                        Err(_) => async move {
                            log::warn!("Unable to download {}", item.url);
                            Ok(())
                        }
                        .boxed_local(),
                    })
            })
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
