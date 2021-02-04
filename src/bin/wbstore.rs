use cancel_culture::{
    cli,
    wayback::{Result, Store},
};
use clap::{crate_authors, crate_version, Clap};
use flate2::{write::GzEncoder, Compression};
use futures::StreamExt;
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
                    match res {
                        Ok((supposed, actual)) => {
                            let items = store.items_by_digest(&supposed).await;
                            let status = items.get(0).and_then(|item| item.status).unwrap_or(0);
                            println!("{},{},{}", supposed, actual, status);
                        }
                        Err(_) => (),
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
    /// Perform a search for a batch of queries from stdin
    #[clap(version = crate_version!(), author = crate_authors!())]
    ComputeDigests,
    Merge(MergeCommand),
    Check(CheckDigest),
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
