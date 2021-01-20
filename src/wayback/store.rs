use super::{Item, Result};

use bytes::Bytes;
use csv::{ReaderBuilder, WriterBuilder};
use data_encoding::BASE32;
use flate2::read::GzDecoder;
use flate2::{Compression, GzBuilder};
use futures_locks::RwLock;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

    pub async fn add(&self, item: &Item, data: Bytes) -> Result<()> {
        let mut contents = self.contents.write().await;

        if !contents.by_digest.contains_key(&item.digest) {
            let file = File::create(self.data_path(&item.digest))?;
            let mut gz = GzBuilder::new()
                .filename(item.infer_filename())
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
            item.mimetype.to_string(),
            item.status_code(),
        ])?;

        contents.file.write_all(&csv.into_inner()?)?;
        contents.file.flush()?;

        Store::add_item_by_url(&mut contents.by_url, item.clone());
        Store::add_item_by_digest(&mut contents.by_digest, item.clone());

        Ok(())
    }

    pub fn compute_digest<R: Read>(input: &mut R) -> Result<String> {
        let mut sha1 = Sha1::new();

        std::io::copy(input, &mut sha1)?;

        let result = sha1.finalize();
        let mut output = String::new();
        BASE32.encode_append(&result, &mut output);

        Ok(output)
    }

    pub fn compute_digest_gz<R: Read>(input: &mut R) -> Result<String> {
        Store::compute_digest(&mut GzDecoder::new(input))
    }

    pub fn compute_item_digest(&self, digest: &str) -> Result<Option<String>> {
        let path = self.data_path(digest);

        if path.is_file() {
            let mut file = File::open(path)?;
            Store::compute_digest_gz(&mut file).map(Some)
        } else {
            Ok(None)
        }
    }

    fn data_path(&self, digest: &str) -> PathBuf {
        self.data_dir().join(format!("{}.gz", digest))
    }

    pub fn read(&self, digest: &str) -> Result<Option<String>> {
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

    pub fn load<P: AsRef<Path>>(base_dir: P) -> Result<Store> {
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
                        Item::from_row(&row.iter().map(|v| v.to_string()).collect::<Vec<_>>())
                    })
                })
                .collect::<Result<Vec<Item>>>()?
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

    pub async fn export<F: Fn(&Item) -> bool, W: Write>(
        &self,
        name: &str,
        out: W,
        f: F,
    ) -> Result<()> {
        let contents = self.contents.read().await;
        let selected = contents.filter(f);

        let mut csv = WriterBuilder::new().from_writer(Vec::with_capacity(selected.len()));

        for item in &selected {
            csv.write_record(&[
                item.url.to_string(),
                item.timestamp(),
                item.digest.to_string(),
                item.mimetype.to_string(),
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
            let file = File::open(path)?;
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
        }

        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::{super::Item, Store};
    use bytes::Bytes;
    use chrono::NaiveDate;
    use flate2::{write::GzEncoder, Compression};
    use std::fs::File;

    fn example_item() -> Item {
        Item::new(
            "https://twitter.com/jdegoes/status/1169217405425455105".to_string(),
            NaiveDate::from_ymd(2019, 9, 16).and_hms(23, 32, 35),
            "AJBB526CEZFOBT3FCQYLRMXQ2MSFHE3O".to_string(),
            "text/html".to_string(),
            Some(200),
        )
    }

    fn new_example_item() -> Item {
        Item::new(
            "https://twitter.com/jdegoes/status/1194638178482700291".to_string(),
            NaiveDate::from_ymd(2019, 11, 13).and_hms(17, 6, 29),
            "ZHYT52YPEOCHJD5FZINSDYXGQZI22WJ4".to_string(),
            "text/html".to_string(),
            Some(200),
        )
    }

    fn fake_item(url: &str) -> Item {
        Item::new(
            url.to_string(),
            NaiveDate::from_ymd(2021, 1, 1).and_hms(12, 0, 0),
            "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ".to_string(),
            "text/html".to_string(),
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
        use fs_extra::dir;

        let store_dir = tempfile::tempdir().unwrap();
        println!("{:?}", store_dir.path());
        fs_extra::copy_items(
            &vec![
                "examples/wayback/store/contents.csv",
                "examples/wayback/store/data/",
            ],
            store_dir.path(),
            &dir::CopyOptions::new(),
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
}
