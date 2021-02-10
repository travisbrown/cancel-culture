mod map;

use super::item::Item;
use futures_locks::RwLock;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid item: {0}")]
    InvalidItem(#[from] super::item::Error),
    #[error("Invalid item: {0}")]
    InvalidItemCsv(#[from] csv::Error),
    #[error("Invalid mapping file: {path:?}")]
    InvalidMappingFile { path: Box<Path> },
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

    pub async fn validate_contents(&self) -> bool {
        false
    }

    pub async fn sizes(&self) -> (usize, usize, usize) {
        (
            self.contents.read().await.by_digest.len(),
            self.invalid.read().await.by_digest.len(),
            self.redirect.read().await.by_digest.len(),
        )
    }
}
