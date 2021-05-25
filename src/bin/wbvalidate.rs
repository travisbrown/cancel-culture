use cancel_culture::{
    cli,
    wayback::{cdx::Client, Store},
};
use clap::{crate_authors, crate_version, Clap};
use flate2::{Compression, GzBuilder};
use std::fs::File;
use std::io::Write;
use std::ops::Deref;
use std::path::Path;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    let store = Store::load(opts.store_dir)?;
    let query = opts.query.unwrap_or_else(|| "".to_string());
    let mut bad_count = 0usize;
    let mut missing_count = 0usize;
    let mut bad_items = vec![];

    let invalid_digest_items = store
        .invalid_digest_items(
            |item| {
                item.url.contains(&query)
                    && item.status != Some(302)
                    && item.digest != "3I42H3S6NNFQ2MSVX7XZKYAYSCX5QBYJ"
            },
            24,
        )
        .await?;

    for (item, broken) in invalid_digest_items {
        if !broken {
            log::info!("Invalid hash, expected {} for {}", item.digest, item.url);
            bad_count += 1;
        } else {
            log::error!("Missing or broken file {} for {}", item.digest, item.url);
            missing_count += 1;
        }
        bad_items.push(item.clone());
    }

    log::warn!("bad: {}, missing: {}", bad_count, missing_count);

    if !bad_items.is_empty() && opts.fix {
        let client = Client::new();
        for item in bad_items {
            let result = client.download(&item, true).await?;
            let actual = Store::compute_digest(&mut result.clone().deref())?;
            log::info!("Downloaded {} for {} for {}", actual, item.digest, item.url);

            if actual == item.digest {
                let path = Path::new(&opts.tmp_dir);
                log::info!("Saving {} to {:?}", actual, path);
                let file = File::create(path.join(format!("{}.gz", actual)))?;
                let mut gz = GzBuilder::new()
                    .filename(item.infer_filename())
                    .write(file, Compression::default());
                gz.write_all(&result)?;
                gz.finish()?;
            }
        }
    }

    Ok(())
}

#[derive(Clap)]
#[clap(name = "wbvalidate", version = crate_version!(), author = crate_authors!())]
struct Opts {
    /// Wayback Machine store directory
    #[clap(short, long, default_value = "wayback")]
    store_dir: String,
    /// Temporary store data directory
    #[clap(short, long, default_value = "tmp-store-data")]
    tmp_dir: String,
    /// Attempt to re-download currently invalid items
    #[clap(short, long)]
    fix: bool,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    query: Option<String>,
}
