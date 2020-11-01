use super::{Item, Result, Store};
use bytes::Bytes;
use futures::{Future, FutureExt, StreamExt, TryFutureExt, TryStreamExt};
use log::info;
use reqwest::Client as RClient;

pub struct Client {
    underlying: RClient,
}

impl Client {
    const CDX_BASE: &'static str = "https://web.archive.org/cdx/search/cdx";
    const CDX_OPTIONS: &'static str =
        "&output=json&fl=original,timestamp,digest,mimetype,statuscode";

    pub fn new() -> Client {
        Client {
            underlying: RClient::new(),
        }
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

        rows.into_iter()
            .skip(1)
            .map(|row| Item::from_row(&row))
            .collect()
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
                    .and_then(move |bytes| store.add(item, bytes))
            })
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}
