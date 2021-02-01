use cancel_culture::{
    cli,
    wayback::{cdx::Client, Result, Store},
};
use clap::{crate_authors, crate_version, Clap};

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    let client = Client::new();

    let raw_items = match opts.command {
        SubCommand::Query(CdxQuery { query }) => client.search(&query).await?,
        SubCommand::Batch => {
            let input = cli::read_stdin()?;
            let mut result = vec![];

            for query in input.lines() {
                result.extend(client.search(&query).await?);
            }

            result
        }
        SubCommand::FromJson => {
            let doc = cli::read_stdin()?;

            Client::load_json(doc.as_bytes())?
        }
    };

    let mut items = raw_items
        .into_iter()
        .filter(|item| {
            item.url.len() < 80
                && item.digest != "6ALZFKKMVFADY2U6KXV5DEOLI2PVWFX4" // This is a generic suspension page
                && item.digest != "3I42H3S6NNFQ2MSVX7XZKYAYSCX5QBYJ" // Another problem page
        })
        .collect::<Vec<_>>();

    items.reverse();

    let store = Store::load(opts.store_dir)?;
    let missing = store.count_missing(&items).await;

    log::info!("Downloading {} of {} items", missing, items.len());

    client
        .save_all(&store, &items, true, opts.parallelism)
        .await?;

    Ok(())
}

#[derive(Clap)]
#[clap(name = "wbdl", version = crate_version!(), author = crate_authors!())]
struct Opts {
    /// Wayback Machine store directory
    #[clap(short, long, default_value = "wayback")]
    store_dir: String,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    /// Number of records to save in parallel
    #[clap(short, long, default_value = "6")]
    parallelism: usize,
    #[clap(subcommand)]
    command: SubCommand,
}

#[derive(Clap)]
enum SubCommand {
    #[clap(version = crate_version!(), author = crate_authors!())]
    Query(CdxQuery),
    /// Perform a search for a batch of queries from stdin
    #[clap(version = crate_version!(), author = crate_authors!())]
    Batch,
    /// Download items given CDX search results
    #[clap(version = crate_version!(), author = crate_authors!())]
    FromJson,
}

/// Perform a search for a single query
#[derive(Clap)]
struct CdxQuery {
    /// CDX search query
    query: String,
}
