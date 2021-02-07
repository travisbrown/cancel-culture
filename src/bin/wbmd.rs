use cancel_culture::{cli, wbm::valid};
use clap::{crate_authors, crate_version, Clap};
use futures::StreamExt;

#[tokio::main]
async fn main() -> valid::Result<()> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    match opts.command {
        SubCommand::List { dir, prefix } => {
            let store = valid::ValidStore::new(dir);
            let paths = store.paths_for_prefix(&prefix.unwrap_or("".to_string()));

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
    List {
        /// Optional prefix
        #[clap(short, long)]
        prefix: Option<String>,
        /// The base directory to check
        dir: String,
    },
    Digests {
        /// Optional prefix
        #[clap(short, long)]
        prefix: Option<String>,
        /// The base directory to check
        dir: String,
    },
}
