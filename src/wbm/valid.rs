use flate2::read::GzDecoder;
use futures::{FutureExt, Stream, TryStreamExt};
use lazy_static::lazy_static;
use std::collections::HashSet;
use std::fs::{read_dir, DirEntry, File};
use std::io::{self, Read};
use std::iter::once;
use std::path::{Path, PathBuf};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unexpected item: {path:?}")]
    Unexpected { path: Box<Path> },
    #[error("Invalid digest or prefix: {0}")]
    InvalidDigest(String),
    #[error("I/O error")]
    IOError(#[from] io::Error),
    #[error("I/O error for {digest}: {error:?}")]
    ItemIOError { digest: String, error: io::Error },
    #[error("Unexpected error while computing digests")]
    DigestComputationError,
}

pub type Result<T> = std::result::Result<T, Error>;

lazy_static! {
    static ref NAMES: HashSet<String> = {
        let mut names = HashSet::new();
        names.extend(('2'..='7').map(|c| c.to_string()));
        names.extend(('A'..='Z').map(|c| c.to_string()));
        names
    };
}

pub struct ValidStore {
    base: Box<Path>,
}

impl ValidStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        ValidStore {
            base: path.as_ref().to_path_buf().into_boxed_path(),
        }
    }

    pub fn create<P: AsRef<Path>>(base: P) -> std::io::Result<Self> {
        let path = base.as_ref();

        for name in NAMES.iter() {
            std::fs::create_dir_all(path.join(name))?;
        }

        Ok(ValidStore {
            base: path.to_path_buf().into_boxed_path(),
        })
    }

    pub fn compute_digests(
        &self,
        prefix: Option<&str>,
        n: usize,
    ) -> impl Stream<Item = Result<(String, String)>> {
        futures::stream::iter(self.paths_for_prefix(prefix.unwrap_or("")))
            .map_ok(|(expected, path)| {
                tokio::spawn(async {
                    let mut file = File::open(path)?;
                    match wayback_rs::digest::compute_digest_gz(&mut file) {
                        Ok(actual) => Ok((expected, actual)),
                        Err(error) => Err(Error::ItemIOError {
                            digest: expected,
                            error,
                        }),
                    }
                })
                .map(|result| match result {
                    Ok(Err(error)) => Err(error),
                    Ok(Ok(value)) => Ok(value),
                    Err(_) => Err(Error::DigestComputationError),
                })
            })
            .try_buffer_unordered(n)
    }

    pub fn paths(&self) -> impl Iterator<Item = Result<(String, PathBuf)>> {
        match read_dir(&self.base).and_then(|it| it.collect::<std::result::Result<Vec<_>, _>>()) {
            Err(error) => Box::new(once(Err(error.into())))
                as Box<dyn Iterator<Item = Result<(String, PathBuf)>>>,
            Ok(mut dirs) => {
                dirs.sort_by_key(|entry| entry.file_name());
                Box::new(
                    dirs.into_iter()
                        .flat_map(|entry| match Self::check_dir_entry(&entry) {
                            Err(error) => Box::new(once(Err(error)))
                                as Box<dyn Iterator<Item = Result<(String, PathBuf)>>>,
                            Ok(first) => match read_dir(entry.path()) {
                                Err(error) => Box::new(once(Err(error.into())))
                                    as Box<dyn Iterator<Item = Result<(String, PathBuf)>>>,
                                Ok(files) => Box::new(files.map(move |result| {
                                    result
                                        .map_err(Error::from)
                                        .and_then(|entry| Self::check_file_entry(&first, &entry))
                                })),
                            },
                        }),
                )
            }
        }
    }

    pub fn paths_for_prefix(
        &self,
        prefix: &str,
    ) -> impl Iterator<Item = Result<(String, PathBuf)>> {
        match prefix.chars().next() {
            None => Box::new(self.paths()),
            Some(first_char) => {
                if Self::is_valid_prefix(prefix) {
                    let first = first_char.to_string();
                    match read_dir(self.base.join(&first)) {
                        Err(error) => Box::new(once(Err(error.into())))
                            as Box<dyn Iterator<Item = Result<(String, PathBuf)>>>,
                        Ok(files) => {
                            let p = prefix.to_string();
                            Box::new(
                                files
                                    .map(move |result| {
                                        result.map_err(Error::from).and_then(|entry| {
                                            Self::check_file_entry(&first, &entry)
                                        })
                                    })
                                    .filter(move |result| match result {
                                        Ok((name, _)) => name.starts_with(&p),
                                        Err(_) => true,
                                    }),
                            )
                        }
                    }
                } else {
                    Box::new(once(Err(Error::InvalidDigest(prefix.to_string()))))
                        as Box<dyn Iterator<Item = Result<(String, PathBuf)>>>
                }
            }
        }
    }

    pub fn check_file_location<P: AsRef<Path>>(
        &self,
        candidate: P,
    ) -> Result<Option<std::result::Result<(String, Box<Path>), (String, String)>>> {
        let path = candidate.as_ref();

        if let Some((name, ext)) = path
            .file_stem()
            .and_then(|os| os.to_str())
            .zip(path.extension().and_then(|os| os.to_str()))
        {
            if Self::is_valid_digest(name) && ext == "gz" {
                if let Some(location) = self.location(name) {
                    if location.is_file() {
                        Ok(None)
                    } else {
                        let mut file = File::open(path)?;
                        let digest = wayback_rs::digest::compute_digest_gz(&mut file)?;

                        if digest == name {
                            Ok(Some(Ok((name.to_string(), location))))
                        } else {
                            Ok(Some(Err((name.to_string(), digest))))
                        }
                    }
                } else {
                    Err(Error::InvalidDigest(name.to_string()))
                }
            } else {
                Err(Error::InvalidDigest(name.to_string()))
            }
        } else {
            Err(Error::InvalidDigest(path.to_string_lossy().into_owned()))
        }
    }

    pub fn location(&self, digest: &str) -> Option<Box<Path>> {
        if Self::is_valid_digest(digest) {
            digest.chars().next().map(|first_char| {
                let path = self
                    .base
                    .join(&first_char.to_string())
                    .join(format!("{}.gz", digest));

                path.into_boxed_path()
            })
        } else {
            None
        }
    }

    pub fn contains(&self, digest: &str) -> bool {
        self.lookup(digest).is_some()
    }

    pub fn lookup(&self, digest: &str) -> Option<Box<Path>> {
        self.location(digest).filter(|path| path.is_file())
    }

    pub fn extract(&self, digest: &str) -> Option<std::io::Result<String>> {
        self.lookup(digest).map(|path| {
            let file = File::open(path)?;
            let mut buffer = String::new();

            GzDecoder::new(file).read_to_string(&mut buffer)?;

            Ok(buffer)
        })
    }

    pub fn extract_bytes(&self, digest: &str) -> Option<std::io::Result<Vec<u8>>> {
        self.lookup(digest).map(|path| {
            let file = File::open(path)?;
            let mut buffer = Vec::new();

            GzDecoder::new(file).read_to_end(&mut buffer)?;

            Ok(buffer)
        })
    }

    fn is_valid_digest(candidate: &str) -> bool {
        candidate.len() == 32 && Self::is_valid_prefix(candidate)
    }

    fn is_valid_prefix(candidate: &str) -> bool {
        candidate.len() <= 32 && candidate.chars().all(|c| NAMES.contains(&c.to_string()))
    }

    fn check_file_entry(first: &str, entry: &DirEntry) -> Result<(String, PathBuf)> {
        if entry.file_type()?.is_file() {
            match entry.path().file_stem().and_then(|os| os.to_str()) {
                None => Err(Error::Unexpected {
                    path: entry.path().into_boxed_path(),
                }),
                Some(name) => {
                    if name.starts_with(&first) {
                        Ok((name.to_string(), entry.path()))
                    } else {
                        Err(Error::Unexpected {
                            path: entry.path().into_boxed_path(),
                        })
                    }
                }
            }
        } else {
            Err(Error::Unexpected {
                path: entry.path().into_boxed_path(),
            })
        }
    }

    fn check_dir_entry(entry: &DirEntry) -> Result<String> {
        if entry.file_type()?.is_dir() {
            match entry.file_name().into_string() {
                Err(_) => Err(Error::Unexpected {
                    path: entry.path().into_boxed_path(),
                }),
                Ok(name) => {
                    if NAMES.contains(&name) {
                        Ok(name)
                    } else {
                        Err(Error::Unexpected {
                            path: entry.path().into_boxed_path(),
                        })
                    }
                }
            }
        } else {
            Err(Error::Unexpected {
                path: entry.path().into_boxed_path(),
            })
        }
    }
}
