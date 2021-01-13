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
    save_tgz(&store, &opts.name, &opts.query).await?;

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
    /// Name of output archive (and file prefix)
    #[clap(short, long)]
    name: String,
    query: String,
}

async fn save_tgz(store: &Store, name: &str, query: &str) -> Result<()> {
    let file = File::create(format!("{}.tgz", name))?;
    let encoder = GzEncoder::new(file, Compression::default());
    store
        .export(name, encoder, |item| item.url.contains(&query))
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use cancel_culture::wayback::Store;
    use flate2::{write::GzEncoder, Compression};

    #[tokio::test]
    async fn test_export() {
        let store = Store::load("examples/wayback/store/").unwrap();
        let mut buffer = vec![];
        let encoder = GzEncoder::new(&mut buffer, Compression::default());
        store
            .export("store-export-test", encoder, |item| {
                item.url.contains("twitter.com/ChiefScientist")
            })
            .await
            .unwrap();

        let expected = std::fs::read("examples/wayback/store-export-test.tgz").unwrap();

        assert_eq!(buffer, expected);
    }
}
