use cancel_culture::{cli, wbm, wbm::digest, wbm::valid};
use clap::{crate_authors, crate_version, Clap};
use futures::StreamExt;
use std::path::Path;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    //valid::Result<()> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    match opts.command {
        SubCommand::Create { dir } => {
            valid::ValidStore::create(dir)?;
        }
        SubCommand::Extract { dir, digest } => {
            let store = valid::ValidStore::new(dir);
            if let Some(result) = store.extract(&digest) {
                println!("{}", result?);
            }
        }
        SubCommand::List { dir, prefix } => {
            let store = valid::ValidStore::new(dir);
            let paths = store.paths_for_prefix(&prefix.unwrap_or_else(|| "".to_string()));

            for result in paths {
                println!("{}", result?.0);
            }
        }
        SubCommand::Digests { dir, prefix } => {
            let store = valid::ValidStore::new(dir);

            let (valid, invalid, broken) = store
                .compute_digests(prefix.as_deref(), opts.parallelism)
                .fold((0, 0, 0), |(valid, invalid, broken), result| async move {
                    match result {
                        Ok((expected, actual)) => {
                            if expected == actual {
                                (valid + 1, invalid, broken)
                            } else {
                                log::error!(
                                    "Invalid digest: expected {}, got {}",
                                    expected,
                                    actual
                                );
                                (valid, invalid + 1, broken)
                            }
                        }
                        Err(error) => {
                            log::error!("Error: {:?}", error);
                            (valid, invalid, broken + 1)
                        }
                    }
                })
                .await;

            log::info!("Valid: {}; invalid: {}; broken: {}", valid, invalid, broken);
        }
        SubCommand::DigestsRaw { dir } => {
            for result in std::fs::read_dir(dir)? {
                let entry = result?;

                if entry.path().is_file() {
                    if let Some(name) = entry.path().file_stem().and_then(|os| os.to_str()) {
                        let mut file = std::fs::File::open(entry.path())?;
                        match digest::compute_digest_gz(&mut file) {
                            Ok(digest) => {
                                println!("{},{}", name, digest);
                            }
                            Err(error) => {
                                log::error!("Error at {}: {:?}", name, error);
                            }
                        }
                    } else {
                        log::info!("Ignoring file: {:?}", entry.path());
                    }
                } else {
                    log::info!("Ignoring directory: {:?}", entry.path());
                }
            }
        }
        SubCommand::RenameRaw { dir, out } => {
            let out_path = Path::new(&out);

            for result in std::fs::read_dir(dir)? {
                let entry = result?;

                if entry.path().is_file() {
                    if let Some(name) = entry.path().file_stem().and_then(|os| os.to_str()) {
                        let mut file = std::fs::File::open(entry.path())?;
                        match digest::compute_digest_gz(&mut file) {
                            Ok(digest) => {
                                std::fs::copy(
                                    entry.path(),
                                    out_path.join(format!("{}.gz", digest)),
                                )?;
                            }
                            Err(error) => {
                                log::error!("Error at {}: {:?}", name, error);
                            }
                        }
                    } else {
                        log::info!("Ignoring file: {:?}", entry.path());
                    }
                } else {
                    log::info!("Ignoring directory: {:?}", entry.path());
                }
            }
        }
        SubCommand::AddFile { dir, input } => {
            let store = valid::ValidStore::new(dir);

            match store.check_file_location(&input)? {
                None => log::warn!("File already exists in store: {}", input),
                Some(Ok((name, location))) => {
                    log::info!("Adding file with digest: {}", name);
                    std::fs::copy(&input, &location)?;

                    println!("{},{}", input, location.to_string_lossy());
                }
                Some(Err((expected, actual))) => {
                    log::error!(
                        "File to add has invalid digest (expected: {}; actual: {}): {}",
                        expected,
                        actual,
                        input
                    );
                }
            }
        }
        SubCommand::DownloadQuery {
            valid,
            other,
            redirect_mapping,
            invalid_mapping,
            query,
        } => {
            let downloader =
                wbm::Downloader::new(valid, other, redirect_mapping, invalid_mapping, "output")?;
            let client = cancel_culture::wayback::cdx::Client::new();

            let results = client.search(&query).await?;

            downloader.save_all(&results).await?;
        }
        SubCommand::Validate { dir } => {
            let s = wbm::store::Store::load(dir)?;

            println!("{:?}", s.sizes().await);
        }
    }

    Ok(())
}

#[derive(Clap)]
#[clap(name = "wbmd", version = crate_version!(), author = crate_authors!())]
struct Opts {
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
    Create {
        /// The base directory
        #[clap(short, long)]
        dir: String,
    },
    Extract {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        // Digest
        digest: String,
    },
    List {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        /// Optional prefix
        #[clap(short, long)]
        prefix: Option<String>,
    },
    Digests {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        /// Optional prefix
        #[clap(short, long)]
        prefix: Option<String>,
    },
    /// Compute all digests for files in a directory
    DigestsRaw {
        /// The directory
        #[clap(short, long)]
        dir: String,
    },
    /// Compute all digests for files in a directory and rename them accordingly
    RenameRaw {
        /// The directory
        #[clap(short, long)]
        dir: String,
        /// The output directory
        #[clap(short, long)]
        out: String,
    },
    AddFile {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        /// The file path to consider adding
        #[clap(short, long)]
        input: String,
    },
    DownloadQuery {
        /// The valid base directory
        #[clap(short, long)]
        valid: String,
        /// The invalid base directory
        #[clap(short, long)]
        other: String,
        /// The redirect mapping file
        #[clap(short, long)]
        redirect_mapping: String,
        /// The redirect mapping file
        #[clap(short, long)]
        invalid_mapping: String,
        /// The query
        query: String,
    },
    Validate {
        /// The base directory
        #[clap(short, long)]
        dir: String,
    },
}
