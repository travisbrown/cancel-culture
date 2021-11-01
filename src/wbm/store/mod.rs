mod map;

use futures::{join, FutureExt};
use futures_locks::RwLock;
use std::collections::HashSet;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid item: {0}")]
    InvalidItem(#[from] wayback_rs::item::Error),
    #[error("Invalid item: {0}")]
    InvalidItemCsv(#[from] csv::Error),
    #[error("Invalid mapping file: {path:?}")]
    InvalidMappingFile { path: Box<Path> },
    #[error("Valid store error: {0:?}")]
    ValidStoreError(#[from] super::valid::Error),
    #[error("I/O error")]
    IOError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Store {
    valid_store: super::valid::ValidStore,
    other_store: super::valid::ValidStore,
    contents: RwLock<map::ItemFileMap>,
    invalid: RwLock<map::MappingFileMap>,
    redirect: RwLock<map::MappingFileMap>,
}

impl Store {
    const DEFAULT_DATA_DIR_NAME: &'static str = "data";
    const DEFAULT_VALID_DIR_NAME: &'static str = "valid";
    const DEFAULT_OTHER_DIR_NAME: &'static str = "other";
    const DEFAULT_CONTENTS_FILE_NAME: &'static str = "contents.csv";
    const DEFAULT_INVALID_FILE_NAME: &'static str = "invalid.csv";
    const DEFAULT_REDIRECT_FILE_NAME: &'static str = "redirect.csv";

    pub fn load<P: AsRef<Path>>(base: P) -> Result<Self> {
        let base_path = base.as_ref();

        Self::load_from_paths(
            base_path
                .join(Self::DEFAULT_DATA_DIR_NAME)
                .join(Self::DEFAULT_VALID_DIR_NAME),
            base_path
                .join(Self::DEFAULT_DATA_DIR_NAME)
                .join(Self::DEFAULT_OTHER_DIR_NAME),
            base_path.join(Self::DEFAULT_CONTENTS_FILE_NAME),
            base_path.join(Self::DEFAULT_INVALID_FILE_NAME),
            base_path.join(Self::DEFAULT_REDIRECT_FILE_NAME),
        )
    }

    fn load_from_paths<
        P1: AsRef<Path>,
        P2: AsRef<Path>,
        P3: AsRef<Path>,
        P4: AsRef<Path>,
        P5: AsRef<Path>,
    >(
        valid_store_path: P1,
        other_store_path: P2,
        contents_path: P3,
        invalid_path: P4,
        redirect_path: P5,
    ) -> Result<Self> {
        Ok(Store {
            valid_store: super::valid::ValidStore::new(valid_store_path),
            other_store: super::valid::ValidStore::new(other_store_path),
            contents: RwLock::new(map::ItemFileMap::load(&contents_path)?),
            invalid: RwLock::new(map::MappingFileMap::load(&invalid_path)?),
            redirect: RwLock::new(map::MappingFileMap::load(&redirect_path)?),
        })
    }

    pub async fn extract_retweets(
        &self,
        known_retweet_status_ids: HashSet<u64>,
    ) -> Vec<((String, u64), (String, u64))> {
        let contents = self.contents.read().await;
        log::info!("Loaded");
        let redirect_items = contents.filter(|item| item.status == Some(302));
        log::info!("Filtered");

        redirect_items
            .into_iter()
            .filter_map(|item| {
                super::util::parse_tweet_url(&item.url).and_then(
                    |(retweeter, retweet_status_id)| {
                        if !known_retweet_status_ids.contains(&retweet_status_id) {
                            match self.valid_store.extract(&item.digest) {
                                Some(Ok(content)) => {
                                    match super::util::parse_tweet_redirect_html(&content) {
                                        Some((tweeter, tweet_status_id)) => {
                                            let pair = (
                                                (retweeter, retweet_status_id),
                                                (tweeter, tweet_status_id),
                                            );
                                            Some(pair)
                                        }
                                        None => {
                                            log::error!(
                                                "Invalid content for {}: {}",
                                                item.digest,
                                                content
                                            );
                                            None
                                        }
                                    }
                                }
                                Some(Err(error)) => {
                                    log::error!(
                                        "Error loading content for {}: {:?}",
                                        item.digest,
                                        error
                                    );
                                    None
                                }
                                None => {
                                    log::warn!("Missing content for {}: {}", item.digest, item.url);
                                    None
                                }
                            }
                        } else {
                            None
                        }
                    },
                )
            })
            .collect()
    }

    /// Generate a script for removing items from valid that don't appear in the contents
    pub async fn clean_valid(&self) -> Result<String> {
        let contents = self.contents.read().await;
        let mut good = 0;
        let mut bad = 0;
        let mut missing = 0;
        let mut output = String::new();

        for result in self.valid_store.paths() {
            let (digest, path) = result?;
            if !contents.contains_digest(&digest) {
                if !self.other_store.contains(&digest) {
                    let target_path =
                        self.other_store
                            .location(&digest)
                            .ok_or(Error::ValidStoreError(super::valid::Error::InvalidDigest(
                                digest,
                            )))?;
                    output.push_str(&format!(
                        "mv {} {}\n",
                        path.as_os_str().to_string_lossy(),
                        target_path.as_os_str().to_string_lossy()
                    ));
                    missing += 1;
                } else {
                    output.push_str(&format!("rm {}\n", path.as_os_str().to_string_lossy()));
                    bad += 1;
                }
            } else {
                good += 1;
            }
        }

        log::info!("Checking valid for representation in contents");
        log::info!("* No action needed: {}", good);
        log::info!("* Deletion needed: {}", bad);
        log::info!("* Move to other needed: {}", missing);

        Ok(output)
    }

    /// Generate a script for removing items from other that also appear in valid
    pub async fn clean_other(&self) -> Result<String> {
        let mut good = 0;
        let mut bad = 0;
        let mut output = String::new();

        for result in self.other_store.paths() {
            let (digest, path) = result?;

            if self.valid_store.contains(&digest) {
                output.push_str(&format!("rm {}\n", path.as_os_str().to_string_lossy()));
                bad += 1;
            } else {
                good += 1;
            }
        }

        log::info!("Checking other for duplicates with valid");
        log::info!("* No action needed: {}", good);
        log::info!("* Deletion needed: {}", bad);

        Ok(output)
    }

    pub async fn validate_contents(&self) -> Result<Vec<String>> {
        let contents = &self.contents.read().await;
        let invalid = &self.invalid.read().await.by_digest;
        let mut valid_count = 0;
        let mut invalid_count = 0;
        let mut missing_count = 0;
        let mut output = vec![];

        for digest in contents.digests() {
            if self.valid_store.contains(digest) {
                valid_count += 1;
            } else if invalid.contains_key(digest) {
                invalid_count += 1;
            } else {
                println!("{}", digest);
                output.push(digest.clone());
                missing_count += 1;
            }
        }

        log::info!("Checking contents for missing items");
        log::info!("* Valid: {}", valid_count);
        log::info!("* Invalid: {}", invalid_count);
        log::info!("* Missing: {}", missing_count);

        Ok(output)
    }

    pub async fn validate_redirect_contents(&self) -> Result<Vec<String>> {
        let contents = &self.contents.read().await;
        let redirect = &self.redirect.read().await.by_digest;
        let mut valid_count = 0;
        let mut output = vec![];

        for digest in contents.digests() {
            let items = contents.items_by_digest(digest);
            if items.iter().any(|item| item.status == Some(302)) {
                if redirect.contains_key(digest) {
                    valid_count += 1;
                } else {
                    output.push(digest.clone());
                }
            }
        }

        log::info!("Checking redirects for missing items");
        log::info!("* Valid: {}", valid_count);
        log::info!("* Missing: {}", output.len());

        Ok(output)
    }

    pub async fn validate_redirects(&self) -> Result<Vec<String>> {
        let mut redirect_values = self
            .redirect
            .read()
            .await
            .by_digest
            .values()
            .cloned()
            .collect::<Vec<String>>();
        redirect_values.sort();
        redirect_values.dedup();

        let mut valid_count = 0;
        let mut other_count = 0;
        let mut output = vec![];

        for digest in redirect_values {
            if self.valid_store.contains(&digest) {
                valid_count += 1;
            } else if self.other_store.contains(&digest) {
                other_count += 1;
            } else {
                output.push(digest.clone());
            }
        }

        log::info!("Checking for missing redirect values");
        log::info!("* Valid: {}", valid_count);
        log::info!("* Other: {}", other_count);
        log::info!("* Missing: {}", output.len());

        Ok(output)
    }

    pub async fn validate_invalids(&self) -> Result<Vec<String>> {
        let mut invalid_values = self
            .invalid
            .read()
            .await
            .by_digest
            .values()
            .cloned()
            .collect::<Vec<String>>();
        invalid_values.sort();
        invalid_values.dedup();

        let mut valid_count = 0;
        let mut other_count = 0;
        let mut output = vec![];

        for digest in invalid_values {
            if self.valid_store.contains(&digest) {
                valid_count += 1;
            } else if self.other_store.contains(&digest) {
                other_count += 1;
            } else {
                output.push(digest.clone());
            }
        }

        log::info!("Checking for missing invalid values");
        log::info!("* Valid: {}", valid_count);
        log::info!("* Other: {}", other_count);
        log::info!("* Missing: {}", output.len());

        Ok(output)
    }

    pub async fn sizes(&self) -> (usize, usize, usize) {
        join!(
            self.contents.read().map(|v| v.items.len()),
            self.invalid.read().map(|v| v.by_digest.len()),
            self.redirect.read().map(|v| v.by_digest.len()),
        )
    }
}
