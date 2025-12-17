use chrono::Utc;
use clap::Parser;
use futures::TryStreamExt;
use futures_locks::Mutex;
use std::fs::File;
use wayback_rs::cdx::IndexClient;

const PAGE_SIZE: usize = 150000;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opts: Opts = Opts::parse();
    let _ = cancel_culture::cli::init_logging(opts.verbose)?;
    let adaptive = cancel_culture::wbm::pacer::adaptive_wayback_pacer();
    let (pacer, observer) = match opts.wayback_pacing {
        cancel_culture::wbm::pacer::WaybackPacingProfile::Adaptive => {
            (adaptive.pacer, Some(adaptive.observer))
        }
        _ => (cancel_culture::wbm::pacer::wayback_pacer(opts.wayback_pacing), None),
    };
    let mut client = IndexClient::default().with_pacer(pacer);
    if let Some(observer) = observer {
        client = client.with_observer(observer);
    }

    let output_path = opts
        .output
        .unwrap_or_else(|| format!("{}.csv", Utc::now().timestamp()));
    let output = Mutex::new(csv::WriterBuilder::new().from_writer(File::create(output_path)?));

    client
        .stream_search(&opts.query, PAGE_SIZE)
        .map_err(Error::from)
        .try_for_each(|item| {
            let output = output.clone();
            async move {
                let mut output = output.lock().await;
                output.write_record(item.to_record())?;

                Ok(())
            }
        })
        .await?;

    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error: {0:?}")]
    Io(#[from] std::io::Error),
    #[error("CDX error: {0:?}")]
    IndexClient(#[from] wayback_rs::cdx::Error),
    #[error("CSV writing error: {0:?}")]
    Csv(#[from] csv::Error),
    #[error("Log initialization error: {0:?}")]
    LogInitialization(#[from] log::SetLoggerError),
}

#[derive(Parser)]
#[clap(name = "cdxdl", version, author)]
struct Opts {
    /// Level of verbosity
    #[clap(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Wayback pacing profile (controls token-bucket pacing for CDX and content requests)
    #[clap(long, value_enum, default_value_t = cancel_culture::wbm::pacer::WaybackPacingProfile::Default)]
    wayback_pacing: cancel_culture::wbm::pacer::WaybackPacingProfile,
    /// Query URL
    #[clap(short, long)]
    query: String,
    /// Output file (defaults to <timestamp>.csv)
    #[clap(short, long)]
    output: Option<String>,
}
