use super::{Error, Result};
use csv::ReaderBuilder;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::Path;
use wayback_rs::Item;

pub(super) struct ItemFileMap {
    pub(super) items: Vec<Item>,
    by_url: HashMap<String, Vec<usize>>,
    by_digest: HashMap<String, Vec<usize>>,
    pub(super) _file: File,
}

impl ItemFileMap {
    pub(super) fn load<P: AsRef<Path>>(path: &P) -> Result<Self> {
        let items = if path.as_ref().is_file() {
            let mut reader = ReaderBuilder::new()
                .has_headers(false)
                .from_reader(File::open(path)?);

            reader
                .records()
                .map(|result| {
                    result.map_err(|error| error.into()).and_then(|row| {
                        Item::parse_optional_record(
                            row.get(0),
                            row.get(1),
                            row.get(2),
                            row.get(3),
                            Some("0"),
                            row.get(4),
                        )
                        .map_err(Error::InvalidItem)
                    })
                })
                .collect::<Result<Vec<Item>>>()?
        } else {
            vec![]
        };

        let mut by_url = HashMap::new();
        let mut by_digest = HashMap::new();

        for (index, item) in items.iter().enumerate() {
            Self::add_item_by_url(&mut by_url, item, index);
            Self::add_item_by_digest(&mut by_digest, item, index);
        }

        let file = OpenOptions::new().append(true).create(true).open(path)?;

        Ok(ItemFileMap {
            items,
            by_url,
            by_digest,
            _file: file,
        })
    }

    pub(super) fn digests(&self) -> impl Iterator<Item = &String> {
        self.by_digest.keys()
    }

    pub(super) fn contains_digest(&self, value: &str) -> bool {
        self.by_digest.contains_key(value)
    }

    pub(super) fn items_by_url(&self, value: &str) -> Vec<&Item> {
        self.by_url
            .get(value)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|index| self.items.get(*index))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn items_by_digest(&self, value: &str) -> Vec<&Item> {
        self.by_digest
            .get(value)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|index| self.items.get(*index))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn add_item_by_url(map: &mut HashMap<String, Vec<usize>>, item: &Item, index: usize) {
        match map.get_mut(&item.url) {
            Some(url_items) => {
                url_items.push(index);
            }
            None => {
                map.insert(item.url.clone(), vec![index]);
            }
        }
    }

    fn add_item_by_digest(map: &mut HashMap<String, Vec<usize>>, item: &Item, index: usize) {
        match map.get_mut(&item.digest) {
            Some(digest_items) => {
                digest_items.push(index);
            }
            None => {
                map.insert(item.digest.clone(), vec![index]);
            }
        }
    }

    pub fn filter<F: Fn(&Item) -> bool>(&self, pred: F) -> Vec<&Item> {
        let mut selected = vec![];

        for item in &self.items {
            if pred(item) {
                selected.push(item);
            }
        }

        selected.sort_by_key(|item| item.url.to_string());

        selected
    }
}

pub(super) struct MappingFileMap {
    pub(super) by_digest: HashMap<String, String>,
    pub(super) file: File,
}

impl MappingFileMap {
    pub(super) fn load<P: AsRef<Path>>(path: &P) -> Result<Self> {
        let by_digest = if path.as_ref().is_file() {
            let reader = BufReader::new(File::open(path)?);

            reader
                .lines()
                .map(|result| {
                    let line = result?;
                    let mut fields = line.split(',');

                    let (first, second) = fields.next().zip(fields.next()).ok_or_else(|| {
                        Error::InvalidMappingFile {
                            path: path.as_ref().to_path_buf().into_boxed_path(),
                        }
                    })?;

                    Ok((first.to_string(), second.to_string()))
                })
                .collect::<Result<_>>()?
        } else {
            HashMap::new()
        };

        let file = OpenOptions::new().append(true).create(true).open(path)?;

        Ok(MappingFileMap { by_digest, file })
    }
}
