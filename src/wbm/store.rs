use crate::browser::twitter::parser::{self, BrowserTweet};
use bytes::Bytes;
use csv::{ReaderBuilder, WriterBuilder};
use data_encoding::BASE32;
use flate2::read::GzDecoder;
use flate2::{Compression, GzBuilder};
use futures::{Future, FutureExt, Stream, StreamExt, TryStreamExt};
use futures_locks::{Mutex, RwLock};
use itertools::Itertools;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use wayback_rs::Item;

use std::fmt::{Debug, Display, Formatter};
use tokio::task::JoinError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    ClientError(#[from] reqwest::Error),
    ItemError(#[from] wayback_rs::item::Error),
    ItemParsingError(String),
    ItemDecodingError(#[from] serde_json::Error),
    FileIOError(#[from] std::io::Error),
    StoreContentsDecodingError(#[from] csv::Error),
    StoreContentsEncodingError(#[from] csv::IntoInnerError<csv::Writer<Vec<u8>>>),
    TaskError(#[from] JoinError),
    DataPathError(PathBuf),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        Debug::fmt(self, f)
    }
}

struct Contents {
    by_url: HashMap<String, Vec<Item>>,
    by_digest: HashMap<String, Vec<Item>>,
    file: File,
}

impl Contents {
    pub fn filter<F: Fn(&Item) -> bool>(&self, f: F) -> Vec<&Item> {
        let mut selected = Vec::with_capacity(self.by_digest.len());

        for items in self.by_digest.values() {
            for item in items {
                if f(item) {
                    selected.push(item);
                }
            }
        }

        selected.sort_by_key(|item| item.url.to_string());

        selected
    }
}

pub struct Store {
    base_dir: PathBuf,
    contents: RwLock<Contents>,
}

impl Store {
    const CONTENTS_FILE_NAME: &'static str = "contents.csv";
    const DATA_DIR_NAME: &'static str = "data";

    pub async fn contains(&self, item: &Item) -> bool {
        let contents = self.contents.read().await;

        if let Some(items) = contents.by_url.get(&item.url) {
            items.contains(item)
        } else {
            false
        }
    }

    pub async fn count_missing(&self, items: &[Item]) -> usize {
        let contents = self.contents.read().await;

        items
            .iter()
            .filter(|item| {
                if let Some(items) = contents.by_url.get(&item.url) {
                    !items.contains(item)
                } else {
                    true
                }
            })
            .count()
    }

    pub async fn items_by_digest(&self, digest: &str) -> Vec<Item> {
        self.contents
            .read()
            .await
            .by_digest
            .get(digest)
            .map(|res| res.to_vec())
            .unwrap_or_default()
    }

    pub async fn add(&self, item: &Item, data: Bytes) -> Result<(), Error> {
        let mut contents = self.contents.write().await;

        if !contents.by_digest.contains_key(&item.digest) {
            let file = File::create(self.data_path(&item.digest))?;
            let mut gz = GzBuilder::new()
                .filename(item.make_filename())
                .write(file, Compression::default());
            gz.write_all(&data)?;
            gz.finish()?;
        }

        if let Some(items) = contents.by_url.get(&item.url) {
            if items.contains(item) {
                return Ok(());
            }
        }

        let mut csv = WriterBuilder::new().from_writer(vec![]);
        csv.write_record(&[
            item.url.to_string(),
            item.timestamp(),
            item.digest.to_string(),
            item.mime_type.to_string(),
            item.status_code(),
        ])?;

        contents.file.write_all(&csv.into_inner()?)?;
        contents.file.flush()?;

        Store::add_item_by_url(&mut contents.by_url, item.clone());
        Store::add_item_by_digest(&mut contents.by_digest, item.clone());

        Ok(())
    }

    pub fn compute_digest<R: Read>(input: &mut R) -> Result<String, Error> {
        let mut sha1 = Sha1::new();

        std::io::copy(input, &mut sha1)?;

        let result = sha1.finalize();
        let mut output = String::new();
        BASE32.encode_append(&result, &mut output);

        Ok(output)
    }

    pub fn compute_digest_gz<R: Read>(input: &mut R) -> Result<String, Error> {
        Store::compute_digest(&mut GzDecoder::new(input))
    }

    pub fn compute_item_digest(&self, digest: &str) -> Result<Option<String>, Error> {
        let path = self.data_path(digest);

        if path.is_file() {
            let mut file = File::open(path)?;
            Store::compute_digest_gz(&mut file).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn check_item_digest(&self, digest: &str) -> bool {
        match self.compute_item_digest(digest) {
            Ok(Some(actual)) => digest == actual,
            _ => false,
        }
    }

    fn data_path(&self, digest: &str) -> PathBuf {
        self.data_dir().join(format!("{}.gz", digest))
    }

    pub fn data_paths(&self) -> Box<dyn Iterator<Item = std::io::Result<PathBuf>>> {
        match fs::read_dir(self.data_dir()) {
            Ok(entries) => Box::new(entries.map(|entry| entry.map(|v| v.path()))),
            Err(error) => Box::new(std::iter::once(Err(error))),
        }
    }

    pub fn read(&self, digest: &str) -> Result<Option<String>, Error> {
        let path = self.data_path(digest);

        if path.is_file() {
            let file = File::open(path)?;
            let mut gz = GzDecoder::new(file);
            let mut res = String::new();
            gz.read_to_string(&mut res)?;
            Ok(Some(res))
        } else {
            Ok(None)
        }
    }

    pub fn extract_digest<P: AsRef<Path>>(path: P) -> Option<String> {
        path.as_ref()
            .file_stem()
            .and_then(|s| s.to_str().map(|s| s.to_owned()))
    }

    pub fn load<P: AsRef<Path>>(base_dir: P) -> Result<Store, Error> {
        let base_dir_path = base_dir.as_ref();

        if !base_dir_path.exists() {
            return Err(Error::DataPathError(base_dir_path.to_path_buf()));
        }

        let data_dir_path = base_dir_path.join(Store::DATA_DIR_NAME);

        if !data_dir_path.exists() {
            std::fs::create_dir(data_dir_path)?;
        }

        let contents_path = Store::contents_path(&base_dir);

        let items = if contents_path.is_file() {
            let contents_file = OpenOptions::new().read(true).open(contents_path.clone())?;
            let mut reader = ReaderBuilder::new()
                .has_headers(false)
                .from_reader(contents_file);

            reader
                .records()
                .map(|record| {
                    record.map_err(|err| err.into()).and_then(|row| {
                        Item::parse_optional_record(
                            row.get(0),
                            row.get(1),
                            row.get(2),
                            row.get(3),
                            Some("0"),
                            row.get(4),
                        )
                        .map_err(Error::from)
                    })
                })
                .collect::<Result<Vec<Item>, Error>>()?
        } else {
            vec![]
        };

        let mut by_url: HashMap<String, Vec<Item>> = HashMap::new();
        let mut by_digest: HashMap<String, Vec<Item>> = HashMap::new();

        for item in items {
            Store::add_item_by_url(&mut by_url, item.clone());
            Store::add_item_by_digest(&mut by_digest, item);
        }

        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(contents_path)?;

        Ok(Store {
            base_dir: base_dir.as_ref().to_path_buf(),
            contents: RwLock::new(Contents {
                by_url,
                by_digest,
                file,
            }),
        })
    }

    pub async fn filter<F: Fn(&Item) -> bool>(&self, f: F) -> Vec<Item> {
        let contents = self.contents.read().await;
        contents.filter(f).into_iter().cloned().collect()
    }

    pub async fn invalid_digest_items<F: Fn(&Item) -> bool>(
        &self,
        f: F,
        limit: usize,
    ) -> Result<Vec<(Item, bool)>, Error> {
        let contents = self.contents.read().await;
        let result = Mutex::new(vec![]);
        let selected = contents.filter(f);

        futures::stream::iter(selected.into_iter().cloned())
            .map(Ok)
            .try_for_each_concurrent(limit, |item| {
                let expected = item.digest.clone();
                let mutex = result.clone();

                let path = self.data_path(&item.digest);

                tokio::spawn(async move {
                    if path.is_file() {
                        if let Ok(mut file) = File::open(path) {
                            if let Ok(actual) = Store::compute_digest_gz(&mut file) {
                                if actual != expected {
                                    let mut res = mutex.lock().await;
                                    res.push((item, false));
                                }
                            } else {
                                let mut res = mutex.lock().await;
                                res.push((item, true));
                            }
                        } else {
                            let mut res = mutex.lock().await;
                            res.push((item, true));
                        }
                    } else {
                        let mut res = mutex.lock().await;
                        res.push((item, true));
                    }
                })
            })
            .await?;

        Ok(result.lock().await.clone())
    }

    /// Compute digests for all data files (ignoring the index and logging issues)
    pub async fn compute_all_digests(&self, parallelism: usize) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = self
            .compute_all_digests_stream(parallelism)
            .filter_map(|result| async { result.ok() })
            .collect()
            .await;

        result.sort_by_key(|(digest, _)| digest.clone());
        result
    }

    pub fn compute_all_digests_stream(
        &self,
        parallelism: usize,
    ) -> impl Stream<Item = std::result::Result<(String, String), String>> {
        let paths = self.data_paths();
        let actions = paths.filter_map(|maybe_path| match maybe_path {
            Err(err) => {
                log::error!("Data path error: {:?}", err);
                None
            }
            Ok(path) => {
                // For error log messages
                if let Some(path_string) = path
                    .file_stem()
                    .and_then(|oss| oss.to_str().map(|s| s.to_string()))
                {
                    if path.is_file() {
                        match File::open(path) {
                            Ok(mut f) => Some(tokio::spawn(async move {
                                (path_string, Store::compute_digest_gz(&mut f))
                            })),
                            Err(error) => {
                                log::error!(
                                    "I/O error opening data file {}: {:?}",
                                    path_string,
                                    error
                                );
                                None
                            }
                        }
                    } else {
                        log::error!("Data item is not a file: {}", path_string);
                        None
                    }
                } else {
                    log::error!("Data item file path error: {:?}", path);
                    None
                }
            }
        });

        futures::stream::iter(actions)
            .buffer_unordered(parallelism)
            .filter_map(|handle| async {
                match handle {
                    Ok((path_string, result)) => match result {
                        Ok(digest) => Some(Ok((path_string, digest))),
                        Err(error) => {
                            log::error!(
                                "Error computing digest for gz file {}: {:?}",
                                path_string,
                                error
                            );
                            Some(Err(path_string))
                        }
                    },
                    Err(error) => {
                        log::error!("Error in digest computation task: {:?}", error);
                        None
                    }
                }
            })
    }

    pub async fn export<F: Fn(&Item) -> bool, W: Write>(
        &self,
        name: &str,
        out: W,
        f: F,
    ) -> Result<(), Error> {
        let contents = self.contents.read().await;
        let selected = contents.filter(f);

        let mut csv = WriterBuilder::new().from_writer(Vec::with_capacity(selected.len()));

        for item in &selected {
            csv.write_record(&[
                item.url.to_string(),
                item.timestamp(),
                item.digest.to_string(),
                item.mime_type.to_string(),
                item.status_code(),
            ])?;
        }

        let metadata = contents.file.metadata()?;
        let csv_data = csv.into_inner()?;

        let mut archive = tar::Builder::new(out);
        let mut csv_header = tar::Header::new_gnu();
        csv_header.set_metadata_in_mode(&metadata, tar::HeaderMode::Deterministic);
        csv_header.set_size(csv_data.len() as u64);

        archive.append_data(
            &mut csv_header,
            format!("{}/contents.csv", name),
            &*csv_data,
        )?;

        for item in selected {
            let path = self.data_path(&item.digest);
            if let Ok(file) = File::open(path) {
                let mut gz = GzDecoder::new(file);
                let mut buffer = vec![];
                gz.read_to_end(&mut buffer)?;

                let mut header = tar::Header::new_gnu();
                header.set_metadata_in_mode(&metadata, tar::HeaderMode::Deterministic);
                header.set_size(buffer.len() as u64);

                archive.append_data(
                    &mut header,
                    format!("{}/data/{}", name, item.digest),
                    &*buffer,
                )?;
            } else {
                log::error!("Failure for item {}", item.digest);
            }
        }

        Ok(())
    }

    fn extract_tweets_from_path<P: AsRef<Path>>(
        p: P,
        mime_type: &str,
    ) -> Result<Vec<BrowserTweet>, Error> {
        let path = p.as_ref();

        if path.is_file() {
            let mut file = File::open(path)?;

            if mime_type == "application/json" {
                let mut doc = String::new();
                let mut gz = GzDecoder::new(file);
                gz.read_to_string(&mut doc)?;

                Ok(match parser::extract_tweet_json(&doc) {
                    Some(tweet) => vec![tweet],
                    None => vec![],
                })
            } else {
                match parser::parse_html_gz(&mut file) {
                    Ok(doc) => Ok(parser::extract_tweets(&doc)),
                    Err(err) => {
                        log::error!("Failed reading {:?}: {:?}", path, err);
                        Ok(vec![])
                    }
                }
            }
        } else {
            Ok(vec![])
        }
    }

    pub fn extract_tweets_stream<'a, I: IntoIterator<Item = Item> + 'a>(
        &'a self,
        items: I,
        limit: usize,
    ) -> impl Stream<Item = Result<(Item, Vec<BrowserTweet>), Error>> + 'a {
        futures::stream::iter(items.into_iter().unique_by(|item| item.digest.clone()))
            .map(move |item| {
                let path = self.data_path(&item.digest);

                Ok(tokio::spawn(async move {
                    Self::extract_tweets_from_path(path, &item.mime_type)
                        .map(|tweets| (item, tweets))
                }))
            })
            .try_buffer_unordered(limit)
            .map(|res| res.map_err(From::from).and_then(|inner| inner))
    }

    pub async fn extract_tweets<F: Fn(&Item) -> bool>(
        &self,
        f: F,
        limit: usize,
    ) -> Result<HashMap<u64, Vec<BrowserTweet>>, Error> {
        let contents = self.contents.read().await;
        let selected = contents.filter(f).into_iter().cloned();

        self.extract_tweets_stream(selected, limit)
            .try_fold(HashMap::new(), |mut acc, tweets| async {
                for tweet in tweets.1 {
                    let entry = acc.entry(tweet.id).or_insert_with(|| Vec::with_capacity(1));
                    entry.push(tweet);
                }
                Ok(acc)
            })
            .await
    }

    fn add_item_by_url(map: &mut HashMap<String, Vec<Item>>, item: Item) {
        match map.get_mut(&item.url) {
            Some(url_items) => {
                url_items.push(item);
            }
            None => {
                map.insert(item.url.clone(), vec![item]);
            }
        }
    }

    fn add_item_by_digest(map: &mut HashMap<String, Vec<Item>>, item: Item) {
        match map.get_mut(&item.digest) {
            Some(digest_items) => {
                digest_items.push(item);
            }
            None => {
                map.insert(item.digest.clone(), vec![item]);
            }
        }
    }

    fn data_dir(&self) -> PathBuf {
        self.base_dir.join(Store::DATA_DIR_NAME)
    }

    fn contents_path<P: AsRef<Path>>(base_dir: &P) -> PathBuf {
        base_dir.as_ref().join(Store::CONTENTS_FILE_NAME)
    }

    /// Return a list of paths from the incoming directory that should be excluded
    pub fn merge_data<P: AsRef<Path>>(
        base_dir: &P,
        incoming_dir: &P,
    ) -> Result<Vec<PathBuf>, Error> {
        let base_contents = Self::dir_contents_map(base_dir)?;
        let incoming_contents = Self::dir_contents_map(incoming_dir)?;
        let mut incoming_digests = incoming_contents.into_iter().collect::<Vec<_>>();
        incoming_digests.sort_by_key(|p| p.0.clone());

        let mut result = vec![];

        for (digest, (path, size)) in incoming_digests {
            // We only ever consider excluding a path if there's a collision
            if let Some((base_path, base_size)) = base_contents.get(&digest) {
                let mut f = File::open(path.clone())?;
                let mut base_f = File::open(base_path)?;
                if !file_diff::diff_files(&mut f, &mut base_f) {
                    let mut gf = File::open(path.clone())?;
                    let mut base_gf = File::open(base_path)?;
                    let base_actual = Store::compute_digest_gz(&mut base_gf)?;

                    if base_actual != digest {
                        let actual = Store::compute_digest_gz(&mut gf)?;

                        if actual != digest {
                            // If neither digest is correct, we always exclude the incoming one if it is smaller
                            if size < *base_size {
                                result.push(path);
                            }
                        }
                    } else {
                        // If the original file has the proper hash, we always exclude the incoming one
                        result.push(path);
                    }
                } else {
                    // If the files are the same we exclude the incoming one
                    result.push(path);
                }
            }
        }

        Ok(result)
    }

    fn dir_contents_map<P: AsRef<Path>>(path: P) -> Result<HashMap<String, (PathBuf, u64)>, Error> {
        std::fs::read_dir(path)?
            .map(|res| {
                res.map_err(From::from).and_then(|entry| {
                    let p = entry.path();
                    let size = entry.metadata()?.len();
                    let digest = p
                        .file_stem()
                        .and_then(|oss| oss.to_str())
                        .ok_or_else(|| Error::DataPathError(p.clone()))?;

                    Ok((digest.to_string(), (p, size)))
                })
            })
            .collect()
    }

    pub fn save_all<'a>(
        &'a self,
        downloader: &'a wayback_rs::Downloader,
        items: &'a [Item],
        check_duplicate: bool,
        limit: usize,
    ) -> impl Future<Output = Result<(), Error>> + 'a {
        futures::stream::iter(items)
            .filter(move |item| self.contains(item).map(|v| !v))
            .map(Ok)
            .try_for_each_concurrent(limit, move |item| {
                if !check_duplicate || !self.check_item_digest(&item.digest) {
                    log::info!("Downloading {}", item.url);
                    downloader
                        .download_item(item)
                        .then(move |bytes_result| match bytes_result {
                            Ok(bytes) => self.add(item, bytes).boxed_local(),
                            Err(_) => async move {
                                log::warn!("Unable to download {}", item.url);
                                Ok(())
                            }
                            .boxed_local(),
                        })
                        .boxed_local()
                } else {
                    log::info!("Skipping {}", item.url);
                    async { Ok(()) }.boxed_local()
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::Store;
    use bytes::Bytes;
    use chrono::NaiveDate;
    use flate2::{write::GzEncoder, Compression};
    use std::fs::File;
    use std::path::PathBuf;
    use wayback_rs::Item;

    fn example_item() -> Item {
        Item::new(
            "https://twitter.com/jdegoes/status/1169217405425455105".to_string(),
            NaiveDate::from_ymd(2019, 9, 16).and_hms(23, 32, 35),
            "AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O".to_string(),
            "text/html".to_string(),
            0,
            Some(200),
        )
    }

    fn new_example_item() -> Item {
        Item::new(
            "https://twitter.com/jdegoes/status/1194638178482700291".to_string(),
            NaiveDate::from_ymd(2019, 11, 13).and_hms(17, 6, 29),
            "ZHYT52YPEOCHJD5FZINSDYXGQZI22WJ4".to_string(),
            "text/html".to_string(),
            0,
            Some(200),
        )
    }

    fn fake_item(url: &str) -> Item {
        Item::new(
            url.to_string(),
            NaiveDate::from_ymd(2021, 1, 1).and_hms(12, 0, 0),
            "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ".to_string(),
            "text/html".to_string(),
            0,
            Some(200),
        )
    }

    // This is an actual example of a Wayback item with an invalid digest.
    fn real_invalid_item() -> Item {
        Item::new(
            "https://twitter.com/jdegoes/status/1305518012984897536".to_string(),
            NaiveDate::from_ymd(2020, 9, 14).and_hms(15, 36, 27),
            "5DECQVIU7Y3F276SIBAKKCRGDMVXJYFV".to_string(),
            "text/html".to_string(),
            0,
            Some(200),
        )
    }

    #[tokio::test]
    async fn test_store_compute_digest() {
        let mut file = File::open("examples/wayback/ZHYT52YPEOCHJD5FZINSDYXGQZI22WJ4").unwrap();
        let result = Store::compute_digest(&mut file).unwrap();

        assert_eq!(result, "ZHYT52YPEOCHJD5FZINSDYXGQZI22WJ4");
    }

    #[tokio::test]
    async fn test_store_compute_digest_gz() {
        let mut file = File::open("examples/wayback/53SGIJNJMTP6S626CVRCHFTX3OEWXB3E.gz").unwrap();
        let result = Store::compute_digest_gz(&mut file).unwrap();

        assert_eq!(result, "53SGIJNJMTP6S626CVRCHFTX3OEWXB3E");
    }

    #[tokio::test]
    async fn test_store_check_item_digest() {
        let store = Store::load("examples/wayback/store/").unwrap();

        assert!(store.check_item_digest("2G3EOT7X6IEQZXKSM3OJJDW6RBCHB7YE"));
        assert!(store.check_item_digest("3KQVYC56SMX4LL6QGQEZZGXMOVNZR2XX"));
        assert!(store.check_item_digest("AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O"));
        assert!(store.check_item_digest("Y2A3M6COP2G6SKSM4BOHC2MHYS3UW22V"));
        assert!(!store.check_item_digest("5DECQVIU7Y3F276SIBAKKCRGDMVXJYFV"));
        assert!(!store.check_item_digest("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"));
    }

    #[tokio::test]
    async fn test_store_items_by_digest() {
        let store = Store::load("examples/wayback/store/").unwrap();
        let result = store
            .items_by_digest("AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O")
            .await;

        assert_eq!(result, vec![example_item()]);
    }

    #[tokio::test]
    async fn test_store_contains() {
        let store = Store::load("examples/wayback/store/").unwrap();

        assert!(store.contains(&example_item()).await);
    }

    #[tokio::test]
    async fn test_store_count_missing() {
        let store = Store::load("examples/wayback/store/").unwrap();
        let items = vec![
            fake_item("foo"),
            example_item(),
            fake_item("bar"),
            fake_item("qux"),
        ];

        assert_eq!(store.count_missing(&items).await, 3);
    }

    #[tokio::test]
    async fn test_store_add() {
        let store_dir = tempfile::tempdir().unwrap();
        fs_extra::copy_items(
            &vec![
                "examples/wayback/store/contents.csv",
                "examples/wayback/store/data/",
            ],
            store_dir.path(),
            &fs_extra::dir::CopyOptions::new(),
        )
        .unwrap();

        let store = Store::load(store_dir.path()).unwrap();
        let new_item_bytes =
            std::fs::read("examples/wayback/ZHYT52YPEOCHJD5FZINSDYXGQZI22WJ4").unwrap();

        store
            .add(&new_example_item(), Bytes::from(new_item_bytes))
            .await
            .unwrap();

        let new_result = store
            .items_by_digest("ZHYT52YPEOCHJD5FZINSDYXGQZI22WJ4")
            .await;

        assert_eq!(new_result, vec![new_example_item()]);

        let old_result = store
            .items_by_digest("AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O")
            .await;

        assert_eq!(old_result, vec![example_item()]);
    }

    #[tokio::test]
    async fn test_store_export() {
        let store = Store::load("examples/wayback/store/").unwrap();
        let mut buffer = vec![];
        let encoder = GzEncoder::new(&mut buffer, Compression::default());
        store
            .export("store-export-test", encoder, |item| {
                item.url.contains("twitter.com/ChiefScientist")
            })
            .await
            .unwrap();

        let expected = std::fs::read("examples/wayback/store-export-test.tgz").unwrap();

        assert_eq!(buffer, expected);
    }

    #[tokio::test]
    async fn test_store_data_paths() {
        let store = Store::load("examples/wayback/store/").unwrap();
        let mut result = store
            .data_paths()
            .collect::<std::io::Result<Vec<PathBuf>>>()
            .unwrap();

        result.sort_by_key(|path| path.to_string_lossy().to_owned().to_string());

        let expected: Vec<PathBuf> = vec![
            PathBuf::from("examples/wayback/store/data/2G3EOT7X6IEQZXKSM3OJJDW6RBCHB7YE.gz"),
            PathBuf::from("examples/wayback/store/data/3KQVYC56SMX4LL6QGQEZZGXMOVNZR2XX.gz"),
            PathBuf::from("examples/wayback/store/data/5DECQVIU7Y3F276SIBAKKCRGDMVXJYFV.gz"),
            PathBuf::from("examples/wayback/store/data/AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O.gz"),
            PathBuf::from("examples/wayback/store/data/Y2A3M6COP2G6SKSM4BOHC2MHYS3UW22V.gz"),
        ];

        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_store_extract_tweets() {
        let store = Store::load("examples/wayback/store/").unwrap();
        let tweets = store
            .extract_tweets(|item| item.url.contains("twitter.com/ChiefScientist"), 8)
            .await
            .unwrap();

        assert_eq!(tweets.len(), 2);
        assert_eq!(
            tweets
                .get(&1302847271688523778)
                .map(|tweets| tweets.iter().map(|tweet| tweet.user_id).collect()),
            Some(vec![258032124])
        );
        assert_eq!(
            tweets
                .get(&1304565662661001216)
                .map(|tweets| tweets.iter().map(|tweet| tweet.user_id).collect()),
            Some(vec![258032124])
        );
    }

    #[tokio::test]
    async fn test_store_invalid_digest_items() {
        let store_dir = tempfile::tempdir().unwrap();
        fs_extra::copy_items(
            &vec![
                "examples/wayback/store/contents.csv",
                "examples/wayback/store/data/",
            ],
            store_dir.path(),
            &fs_extra::dir::CopyOptions::new(),
        )
        .unwrap();

        let store = Store::load(store_dir.path()).unwrap();
        let new_item_bytes = Bytes::from(
            std::fs::read("examples/wayback/ZHYT52YPEOCHJD5FZINSDYXGQZI22WJ4").unwrap(),
        );

        store
            .add(&new_example_item(), new_item_bytes.clone())
            .await
            .unwrap();

        store.add(&fake_item("foo"), new_item_bytes).await.unwrap();

        let mut result = store.invalid_digest_items(|_| true, 2).await.unwrap();
        result.sort_by_key(|(item, _)| item.url.clone());

        assert_eq!(
            result,
            vec![(fake_item("foo"), false), (real_invalid_item(), false)]
        );
    }

    #[tokio::test]
    async fn test_store_compute_all_digests() {
        let store = Store::load("examples/wayback/store/").unwrap();
        let result = store.compute_all_digests(4).await;
        let expected = vec![
            (
                "2G3EOT7X6IEQZXKSM3OJJDW6RBCHB7YE".to_string(),
                "2G3EOT7X6IEQZXKSM3OJJDW6RBCHB7YE".to_string(),
            ),
            (
                "3KQVYC56SMX4LL6QGQEZZGXMOVNZR2XX".to_string(),
                "3KQVYC56SMX4LL6QGQEZZGXMOVNZR2XX".to_string(),
            ),
            (
                "5DECQVIU7Y3F276SIBAKKCRGDMVXJYFV".to_string(),
                "5BPR3OBK6O7KJ6PKFNJRNUICXWNZ46QG".to_string(),
            ),
            (
                "AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O".to_string(),
                "AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O".to_string(),
            ),
            (
                "Y2A3M6COP2G6SKSM4BOHC2MHYS3UW22V".to_string(),
                "Y2A3M6COP2G6SKSM4BOHC2MHYS3UW22V".to_string(),
            ),
        ];

        assert_eq!(result, expected);
    }
}
