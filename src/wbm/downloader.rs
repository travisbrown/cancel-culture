use super::valid::ValidStore;
use bytes::Bytes;
use reqwest::{redirect, Client};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use wayback_rs::Item;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid mapping file: {path:?}")]
    InvalidMappingFile { path: Box<Path> },
    #[error("I/O error")]
    IOError(#[from] io::Error),
    #[error("HTTP client error")]
    ClientError(#[from] reqwest::Error),
}

const DOWNLOAD_RETRIES: u32 = 8;
const DOWNLOAD_RETRY_BACKOFF_BASE: Duration = Duration::from_millis(250);

pub type Result<T> = std::result::Result<T, Error>;

pub struct Downloader {
    client: Client,
    redirect_client: Client,
    valid_store: ValidStore,
    other_store: ValidStore,
    redirect_mappings: HashMap<String, String>,
    invalid_mappings: HashMap<String, String>,
    _output: PathBuf,
}

impl Downloader {
    pub fn new<
        P1: AsRef<Path>,
        P2: AsRef<Path>,
        P3: AsRef<Path>,
        P4: AsRef<Path>,
        P5: AsRef<Path>,
    >(
        valid_store_path: P1,
        other_store_path: P2,
        redirect_mappings_path: P3,
        invalid_mappings_path: P4,
        output_path: P5,
    ) -> Result<Self> {
        let redirect_mappings = Self::read_mappings(&redirect_mappings_path)?;
        let invalid_mappings = Self::read_mappings(&invalid_mappings_path)?;

        Ok(Downloader {
            client: Client::builder()
                .tcp_keepalive(Some(Duration::from_secs(20)))
                .redirect(redirect::Policy::none())
                .build()
                .unwrap(),
            redirect_client: Client::builder()
                .tcp_keepalive(Some(Duration::from_secs(20)))
                .build()
                .unwrap(),
            valid_store: ValidStore::new(valid_store_path),
            other_store: ValidStore::new(other_store_path),
            redirect_mappings,
            invalid_mappings,
            _output: output_path.as_ref().to_path_buf(),
        })
    }

    pub async fn download(
        &self,
        item: &Item,
        original: bool,
        follow_redirects: bool,
    ) -> Result<Bytes> {
        let client = if follow_redirects {
            &self.redirect_client
        } else {
            &self.client
        };
        Ok(client
            .get(&item.wayback_url(original))
            .send()
            .await?
            .bytes()
            .await?)
    }

    pub async fn download_with_retries(
        &self,
        item: &Item,
        original: bool,
        follow_redirects: bool,
    ) -> Result<Bytes> {
        tryhard::retry_fn(move || self.download(item, original, follow_redirects))
            .retries(DOWNLOAD_RETRIES)
            .exponential_backoff(DOWNLOAD_RETRY_BACKOFF_BASE)
            .await
    }

    pub async fn save_all(&self, items: &[Item]) -> Result<()> {
        let mut known_valid_count = 0;
        let mut known_other = vec![];
        let mut known_redirect_count = 0;
        let mut known_invalid_count = 0;
        let mut unknown = vec![];

        for item in items {
            if self.valid_store.contains(&item.digest) {
                known_valid_count += 1;
            } else if self.other_store.contains(&item.digest) {
                known_other.push(item);
            } else {
                let mut in_mapping = false;

                if self.invalid_mappings.contains_key(&item.digest) {
                    known_invalid_count += 1;
                    in_mapping = true;
                }

                if self.redirect_mappings.contains_key(&item.digest) {
                    known_redirect_count += 1;
                    in_mapping = true;
                }

                if !in_mapping {
                    unknown.push(item);
                }
            }
        }

        log::info!(
            "Known valid: {}\nKnown other: {}\nKnown redirect: {}\nKnown invalid: {}\nUnknown: {}",
            known_valid_count,
            known_other.len(),
            known_redirect_count,
            known_invalid_count,
            unknown.len(),
        );

        Ok(())
    }

    fn read_mappings<P: AsRef<Path>>(path: &P) -> Result<HashMap<String, String>> {
        let reader = BufReader::new(File::open(path)?);

        reader
            .lines()
            .map(|result| {
                let line = result?;
                let mut fields = line.split(',');

                let (first, second) =
                    fields
                        .next()
                        .zip(fields.next())
                        .ok_or_else(|| Error::InvalidMappingFile {
                            path: path.as_ref().to_path_buf().into_boxed_path(),
                        })?;

                Ok((first.to_string(), second.to_string()))
            })
            .collect()
    }
}
