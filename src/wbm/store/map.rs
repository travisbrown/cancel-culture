use super::{super::item::Item, Error, Result};
use csv::{ReaderBuilder, WriterBuilder};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::Path;

pub(super) struct ItemFileMap {
    pub(super) by_url: HashMap<String, Vec<Item>>,
    pub(super) by_digest: HashMap<String, Vec<Item>>,
    pub(super) file: File,
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
                        Item::parse_optional(
                            row.get(0),
                            row.get(1),
                            row.get(2),
                            row.get(3),
                            row.get(4),
                        )
                        .map_err(|error| Error::InvalidItem(error))
                    })
                })
                .collect::<Result<Vec<Item>>>()?
        } else {
            vec![]
        };

        let mut by_url = HashMap::new();
        let mut by_digest = HashMap::new();

        for item in items {
            Self::add_item_by_url(&mut by_url, item.clone());
            Self::add_item_by_digest(&mut by_digest, item);
        }

        let file = OpenOptions::new().append(true).create(true).open(path)?;

        Ok(ItemFileMap {
            by_url,
            by_digest,
            file,
        })
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
                    let mut fields = line.split(",");

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
