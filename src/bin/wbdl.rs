use cancel_culture::{
    cli,
    wayback::{cdx::Client, Store},
};
use clap::{crate_authors, crate_version, Clap};

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    let client = Client::new();
    let items = client
        .search(&opts.query)
        .await?
        .into_iter()
        .filter(|item| item.url.len() < 80)
        .collect::<Vec<_>>();
    log::info!("{} items to download", items.len());

    let store = Store::load(opts.store_dir)?;
    client.save_all(&store, &items, opts.parallelism).await?;

    Ok(())
}

#[derive(Clap)]
#[clap(name = "wbdl", version = crate_version!(), author = crate_authors!())]
struct Opts {
    /// TOML file containing Twitter API keys
    #[clap(short, long, default_value = "wayback")]
    store_dir: String,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    /// Number of records to save in parallel
    #[clap(short, long, default_value = "4")]
    parallelism: usize,
    query: String,
}
