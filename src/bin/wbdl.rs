use cancel_culture::{
    cli,
    wayback::{cdx::Client, Result, Store},
};
use clap::{crate_authors, crate_version, Clap};
use std::fs::File;

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    let client = Client::new();

    let raw_items = match &opts.input_json {
        Some(input_json_path) => {
            let file = File::open(input_json_path)?;

            Client::load_json(file)?
        }
        None => {
            client
                .search(&opts.query.clone().unwrap_or("".to_string()))
                .await?
        }
    };

    let mut items = raw_items
        .into_iter()
        .filter(|item| item.url.len() < 80)
        .collect::<Vec<_>>();

    items.reverse();

    let store = Store::load(opts.store_dir)?;
    let missing = store.count_missing(&items).await;

    log::info!(
        "Downloading {} of {} items for \"{}\"",
        missing,
        items.len(),
        opts.query.or(opts.input_json).unwrap_or("".to_string())
    );

    client.save_all(&store, &items, opts.parallelism).await?;

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
    #[clap(short, long, default_value = "4")]
    parallelism: usize,
    /// Optional JSON file of CDX results
    #[clap(short, long)]
    input_json: Option<String>,
    query: Option<String>,
}
