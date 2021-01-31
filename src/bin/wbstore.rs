use cancel_culture::{
    cli,
    wayback::{Result, Store},
};
use clap::{crate_authors, crate_version, Clap};
use flate2::{write::GzEncoder, Compression};
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
            store.compute_all_digests(opts.parallelism).await;
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
