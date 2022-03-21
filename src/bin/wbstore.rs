use cancel_culture::{
    cli,
    wbm::store::{Error, Store},
};
use clap::Parser;
use flate2::{write::GzEncoder, Compression, GzBuilder};
use futures::StreamExt;
use std::collections::HashSet;
use std::fs::File;
use wayback_rs::Item;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose).unwrap();

    let store = Store::load(opts.store_dir)?;

    match opts.command {
        SubCommand::Export(ExportQuery { name, query }) => {
            save_export_tgz(&store, &name, &query).await?
        }
        SubCommand::ComputeDigests => {
            store
                .compute_all_digests_stream(opts.parallelism)
                .for_each(|res| async {
                    if let Ok((supposed, actual)) = res {
                        let items = store.items_by_digest(&supposed).await;
                        let status = items.get(0).and_then(|item| item.status).unwrap_or(0);
                        println!("{},{},{}", supposed, actual, status);
                    }
                })
                .await;
        }
        SubCommand::ComputeDigestsRaw => {
            store
                .compute_all_digests_stream(opts.parallelism)
                .for_each(|res| async {
                    if let Ok((supposed, actual)) = res {
                        println!("{},{}", supposed, actual);
                    }
                })
                .await;
        }
        SubCommand::Merge(MergeCommand { base, incoming }) => {
            let exclusions = Store::merge_data(&base, &incoming)?;
            for exclusion in exclusions {
                match exclusion.into_os_string().into_string() {
                    Ok(p) => println!("{}", p),
                    Err(e) => log::error!("Merge failure: {:?}", e),
                }
            }
        }
        SubCommand::Check(CheckDigest { value }) => {
            if let Some(actual) = store.compute_item_digest(&value)? {
                if actual == value {
                    log::info!("{} has the correct digest", value);
                } else {
                    log::error!("{} has incorrect value {}", value, actual);
                }
            } else {
                log::warn!("{} does not exist", value);
            }
        }
        SubCommand::ListValid(CheckValidCommand { dir }) => {
            use std::fs::read_dir;

            let mut sub_dirs = read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
            sub_dirs.sort_by_key(|entry| entry.file_name());
            let mut dir_names = HashSet::new();
            dir_names.extend(('2'..='7').map(|c| c.to_string()));
            dir_names.extend(('A'..='Z').map(|c| c.to_string()));

            for entry in sub_dirs {
                let name = entry.file_name().into_string().unwrap();
                if entry.file_type()?.is_dir() {
                    if dir_names.contains(&name) {
                        let files = read_dir(entry.path())?;

                        for file_result in files {
                            let file_entry = file_result?;
                            let file_name = file_entry.file_name().into_string().unwrap();

                            if file_entry.file_type()?.is_file() {
                                if file_name.starts_with(&name) {
                                    match file_entry.path().file_stem().and_then(|os| os.to_str()) {
                                        Some(stem) => println!("{}", stem),
                                        None => {
                                            log::error!("Skipping invalid file name: {}", file_name)
                                        }
                                    }
                                } else {
                                    log::error!("Skipping invalid file name: {}", file_name);
                                }
                            } else {
                                log::error!("Expected directory, found: {}", file_name);
                            }
                        }
                    } else {
                        log::error!("Unexpected directory: {}", name);
                    }
                } else {
                    log::error!("Expected directory, found file: {}", name);
                }
            }
        }
        SubCommand::CheckValid(CheckValidCommand { dir }) => {
            use std::fs::read_dir;

            let mut sub_dirs = read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
            sub_dirs.sort_by_key(|entry| entry.file_name());
            let mut dir_names = HashSet::new();
            dir_names.extend(('2'..='7').map(|c| c.to_string()));
            dir_names.extend(('A'..='Z').map(|c| c.to_string()));

            let mut valid = 0;
            let mut invalid = 0;

            for entry in sub_dirs {
                let name = entry.file_name().into_string().unwrap();
                log::info!("Checking: {}", name);
                if entry.file_type()?.is_dir() {
                    if dir_names.contains(&name) {
                        let files = read_dir(entry.path())?;

                        for file_result in files {
                            let file_entry = file_result?;
                            let file_name = file_entry.file_name().into_string().unwrap();

                            if file_entry.file_type()?.is_file() {
                                if file_name.starts_with(&name) {
                                    let mut file = File::open(file_entry.path())?;
                                    match Store::compute_digest_gz(&mut file) {
                                        Ok(actual) => {
                                            let expected = format!("{}.gz", actual);

                                            if file_name != expected {
                                                invalid += 1;
                                                log::error!("Invalid file: {}/{}", name, file_name);
                                            } else {
                                                valid += 1;
                                            }
                                        }
                                        Err(error) => {
                                            log::error!(
                                                "Error reading file: {} ({:?})",
                                                file_name,
                                                error
                                            );
                                        }
                                    }
                                } else {
                                    log::error!("Skipping invalid file name: {}", file_name);
                                }
                            } else {
                                log::error!("Expected directory, found: {}", file_name);
                            }
                        }
                    } else {
                        log::error!("Unexpected directory: {}", name);
                    }
                } else {
                    log::error!("Expected directory, found file: {}", name);
                }
            }

            log::info!("Valid: {}; invalid: {}", valid, invalid);
        }
        SubCommand::Digest => {
            let content = cli::read_stdin()?;
            let mut bytes = content.as_bytes();
            let digest = Store::compute_digest(&mut bytes)?;
            println!("{}", digest);
        }
    }

    log::logger().flush();

    Ok(())
}

#[derive(Parser)]
#[clap(name = "wbstore", version, author)]
struct Opts {
    /// Wayback Machine store directory
    #[clap(short, long, default_value = "wayback")]
    store_dir: String,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    /// Level of parallelism
    #[clap(short, long, default_value = "6")]
    parallelism: usize,
    #[clap(subcommand)]
    command: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    Export(ExportQuery),
    /// Compute digest for all files in the store's data directory
    ComputeDigests,
    ComputeDigestsRaw,
    Merge(MergeCommand),
    Check(CheckDigest),
    /// Compute digest for the input from stdin
    Digest,
    CheckValid(CheckValidCommand),
    ListValid(CheckValidCommand),
}

/// Export an archive for items whose URL contains the query string
#[derive(Parser)]
struct ExportQuery {
    /// Name of output archive (and file prefix)
    #[clap(short, long)]
    name: String,
    /// URL search query
    query: String,
}

/// Merge two data directories
#[derive(Parser)]
struct MergeCommand {
    /// Base directory
    #[clap(short, long)]
    base: String,
    /// Incoming directory
    #[clap(short, long)]
    incoming: String,
}

/// Check a single digest
#[derive(Parser)]
struct CheckDigest {
    /// Digest to check
    value: String,
}

/// Re-download broken files
#[derive(Parser)]
struct FixCommand {
    /// Base directory for temporary storage
    #[clap(short, long)]
    base: String,
    /// Known digest file
    #[clap(short, long)]
    known: Option<String>,
}

/// Check a directory of known valid files
#[derive(Parser)]
struct CheckValidCommand {
    /// Base directory
    #[clap(short, long)]
    dir: String,
}

async fn save_export_tgz(store: &Store, name: &str, query: &str) -> Result<(), Error> {
    let file = File::create(format!("{}.tgz", name))?;
    let encoder = GzEncoder::new(file, Compression::default());
    store
        .export(name, encoder, |item| {
            item.url.to_lowercase().contains(&query.to_lowercase())
        })
        .await?;

    Ok(())
}

fn save_contents_gz(item: &Item, base: &str, content: &[u8]) -> Result<(), Error> {
    use std::io::Write;

    log::info!("Saving {} to {:?} ({})", item.digest, base, item.url);
    let file = File::create(std::path::Path::new(base).join(format!("{}.gz", item.digest)))?;
    let mut gz = GzBuilder::new()
        .filename(item.make_filename())
        .write(file, Compression::default());
    gz.write_all(content)?;
    gz.finish()?;
    Ok(())
}
