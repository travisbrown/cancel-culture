use super::{Item, Result, Store};
use bytes::Bytes;
use flate2::{Compression, GzBuilder};
use futures::{Future, FutureExt, StreamExt, TryStreamExt};
use log::info;
use reqwest::{redirect, Client as RClient};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::ops::Deref;
use std::path::Path;
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

    pub fn new_without_redirects() -> Client {
        Client {
            underlying: RClient::builder()
                .tcp_keepalive(Some(Duration::from_secs(20)))
                .redirect(redirect::Policy::none())
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
        Ok(self
            .underlying
            .get(&item.wayback_url(original))
            .send()
            .await?
            .bytes()
            .await?)
    }

    // Download file to a subdirectory of the given directory based on whether its digest matches
    pub async fn download_gz_to_dir<P: AsRef<Path>>(&self, base: &P, item: &Item) -> Result<()> {
        let mut good_dir = base.as_ref().to_path_buf();
        good_dir.push("good");
        let mut bad_dir = base.as_ref().to_path_buf();
        bad_dir.push("bad");

        if !good_dir.is_dir() {
            std::fs::create_dir(&good_dir)?;
        }

        if !bad_dir.is_dir() {
            std::fs::create_dir(&bad_dir)?;
        }

        let result = tryhard::retry_fn(move || self.download(item, true))
            .retries(7)
            .exponential_backoff(Duration::from_millis(250))
            .await?;

        let actual = Store::compute_digest(&mut result.deref())?;

        if actual == item.digest {
            log::info!("Saving {} to {:?} ({})", actual, good_dir, item.url);
            let file = File::create(good_dir.join(format!("{}.gz", actual)))?;
            let mut gz = GzBuilder::new()
                .filename(item.infer_filename())
                .write(file, Compression::default());
            gz.write_all(&result)?;
            gz.finish()?;
        } else {
            log::info!("Saving {} to {:?} ({})", item.digest, bad_dir, item.url);
            let file = File::create(bad_dir.join(format!("{}.gz", item.digest)))?;
            let mut gz = GzBuilder::new()
                .filename(item.infer_filename())
                .write(file, Compression::default());
            gz.write_all(&result)?;
            gz.finish()?;
        }

        Ok(())
    }

    pub fn save_all<'a>(
        &'a self,
        store: &'a Store,
        items: &'a [Item],
        check_duplicate: bool,
        limit: usize,
    ) -> impl Future<Output = Result<()>> + 'a {
        futures::stream::iter(items)
            .filter(move |item| store.contains(&item).map(|v| !v))
            .map(Ok)
            .try_for_each_concurrent(limit, move |item| {
                if !check_duplicate || !store.check_item_digest(&item.digest) {
                    info!("Downloading {}", item.url);
                    tryhard::retry_fn(move || self.download(item, true))
                        .retries(7)
                        .exponential_backoff(Duration::from_millis(250))
                        .then(move |bytes_result| match bytes_result {
                            Ok(bytes) => store.add(item, bytes).boxed_local(),
                            Err(_) => async move {
                                log::warn!("Unable to download {}", item.url);
                                Ok(())
                            }
                            .boxed_local(),
                        })
                        .boxed_local()
                } else {
                    info!("Skipping {}", item.url);
                    async { Ok(()) }.boxed_local()
                }
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
