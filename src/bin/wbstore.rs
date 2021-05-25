use cancel_culture::{
    cli,
    wayback::{cdx::Client, Item, Result, Store},
};
use clap::{crate_authors, crate_version, Clap};
use flate2::{write::GzEncoder, Compression, GzBuilder};
use futures::StreamExt;
use std::collections::HashSet;
use std::fs::File;

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

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
        SubCommand::FixRedirects(FixCommand { base }) => {
            let client = Client::new_without_redirects();

            let items = store.filter(|item| item.status == Some(302)).await;

            for item in items {
                if !item.url.to_lowercase().contains("cernovich")
                    && item.digest != "6Q4HKTYOVX4E7HQUF6TXAC4UUG2M227A"
                    && item.digest != "ZBWFUJ2IKMYHPV6ER3CUG6F7GTDKSGVE"
                {
                    let existence_check = std::path::Path::new(&base)
                        .join("good")
                        .join(format!("{}.gz", item.digest));
                    if !existence_check.exists() {
                        match client.download_gz_to_dir(&base, &item).await {
                            Ok(_) => (),
                            Err(err) => log::error!("Problem: {:?}", err),
                        }
                    }
                }
            }
        }
        SubCommand::GuessRedirects(FixCommand { base }) => {
            let items = store.filter(|item| item.status == Some(302)).await;
            let canonical_re = regex::Regex::new(
                r#"<link rel=.canonical. href=.https...twitter.com.([^/]+)/status/(\d+)"#,
            )
            .unwrap();
            let permalink_re = regex::Regex::new(r"data-permalink-path=./([^/]+)").unwrap();
            let fallback_re = regex::Regex::new(r#"<link rel=.canonical. href=.([^"]+)"#).unwrap();

            for item in items {
                if !item.url.to_lowercase().contains("cernovich") {
                    if let Ok(Some(content)) = store.read(&item.digest) {
                        if let Some(canonical_match) = canonical_re.captures_iter(&content).next() {
                            let canonical_screen_name = canonical_match.get(1).unwrap().as_str();
                            let canonical_id = canonical_match.get(2).unwrap().as_str();

                            let screen_name = permalink_re
                                .captures_iter(&content)
                                .filter_map(|m| {
                                    let psn = m.get(1).unwrap().as_str().to_string();
                                    if psn.to_lowercase() == canonical_screen_name.to_lowercase() {
                                        Some(psn)
                                    } else {
                                        None
                                    }
                                })
                                .next()
                                .unwrap_or_else(|| canonical_screen_name.to_string());

                            let new_content = format!(
                          "<html><body>You are being <a href=\"https://twitter.com/{}/status/{}\">redirected</a>.</body></html>",
                          screen_name,
                          canonical_id
                        );
                            let mut ncb = new_content.as_bytes();

                            let guess_digest = Store::compute_digest(&mut ncb)?;

                            if guess_digest == item.digest {
                                save_contents_gz(&item, &base, new_content.as_bytes())?;
                            }
                        } else if let Some(canonical_match) =
                            fallback_re.captures_iter(&content).next()
                        {
                            let new_content = format!(
                              "<html><body>You are being <a href=\"{}\">redirected</a>.</body></html>",
                              canonical_match.get(1).unwrap().as_str()
                            );
                            let mut ncb = new_content.as_bytes();

                            let guess_digest = Store::compute_digest(&mut ncb)?;

                            if guess_digest == item.digest {
                                save_contents_gz(&item, &base, new_content.as_bytes())?;
                            }
                        }
                    }
                }
            }
        }
        SubCommand::Fix(FixCommand { base }) => {
            let throttled_error_digest = "VU34ZWVLIWSRGLOVRZXIJGZXTWX54UOW";
            let error_503_digest = "N67J36CWSVSGPQLJCVMHS3EG7Q4S5VNW";
            let error_504_01_digest = "B575DWBDMQ22WKVZHPROOX4ZLEF3IRNA";
            let error_504_02_digest = "GJIF3BEPWGUMFCQBBTKJ36KZZE5DZLVJ";
            let known_bad = vec![
                throttled_error_digest,
                error_503_digest,
                error_504_01_digest,
                error_504_02_digest,
            ]
            .iter()
            .map(|digest| digest.to_string())
            .collect::<HashSet<_>>();

            let client = Client::new();

            store
                .compute_all_digests_stream(opts.parallelism)
                .zip(futures::stream::iter(0..))
                .for_each_concurrent(4, |(res, i)| {
                    if i % 100 == 0 {
                        log::info!("At item index {}", i);
                    }
                    async {
                        match res {
                            Ok((supposed, actual)) => {
                                if supposed != actual && known_bad.contains(&actual) {
                                    let items = store.items_by_digest(&supposed).await;

                                    for item in items {
                                        match client.download_gz_to_dir(&base, &item).await {
                                            Ok(_) => (),
                                            Err(err) => log::error!("Problem: {:?}", err),
                                        }
                                    }
                                }
                            }
                            Err(digest) => {
                                let items = store.items_by_digest(&digest).await;

                                for item in items {
                                    client.download_gz_to_dir(&base, &item).await.unwrap();
                                }
                            }
                        }
                    }
                })
                .await;
        }
    }

    Ok(())
}

#[derive(Clap)]
#[clap(name = "wbstore", version = crate_version!(), author = crate_authors!())]
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

#[derive(Clap)]
enum SubCommand {
    #[clap(version = crate_version!(), author = crate_authors!())]
    Export(ExportQuery),
    /// Compute digest for all files in the store's data directory
    #[clap(version = crate_version!(), author = crate_authors!())]
    ComputeDigests,
    ComputeDigestsRaw,
    Merge(MergeCommand),
    Check(CheckDigest),
    Fix(FixCommand),
    FixRedirects(FixCommand),
    GuessRedirects(FixCommand),
    /// Compute digest for the input from stdin
    #[clap(version = crate_version!(), author = crate_authors!())]
    Digest,
    CheckValid(CheckValidCommand),
    ListValid(CheckValidCommand),
}

/// Export an archive for items whose URL contains the query string
#[derive(Clap)]
struct ExportQuery {
    /// Name of output archive (and file prefix)
    #[clap(short, long)]
    name: String,
    /// URL search query
    query: String,
}

/// Merge two data directories
#[derive(Clap)]
struct MergeCommand {
    /// Base directory
    #[clap(short, long)]
    base: String,
    /// Incoming directory
    #[clap(short, long)]
    incoming: String,
}

/// Check a single digest
#[derive(Clap)]
struct CheckDigest {
    /// Digest to check
    value: String,
}

/// Re-download broken files
#[derive(Clap)]
struct FixCommand {
    /// Base directory for temporary storage
    #[clap(short, long)]
    base: String,
}

/// Check a directory of known valid files
#[derive(Clap)]
struct CheckValidCommand {
    /// Base directory
    #[clap(short, long)]
    dir: String,
}

async fn save_export_tgz(store: &Store, name: &str, query: &str) -> Result<()> {
    let file = File::create(format!("{}.tgz", name))?;
    let encoder = GzEncoder::new(file, Compression::default());
    store
        .export(name, encoder, |item| {
            item.url.to_lowercase().contains(&query.to_lowercase())
        })
        .await?;

    Ok(())
}

fn save_contents_gz(item: &Item, base: &str, content: &[u8]) -> Result<()> {
    use std::io::Write;

    log::info!("Saving {} to {:?} ({})", item.digest, base, item.url);
    let file = File::create(std::path::Path::new(base).join(format!("{}.gz", item.digest)))?;
    let mut gz = GzBuilder::new()
        .filename(item.infer_filename())
        .write(file, Compression::default());
    gz.write_all(&content)?;
    gz.finish()?;
    Ok(())
}
