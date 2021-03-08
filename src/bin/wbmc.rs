use cancel_culture::{cli, wbm, wbm::digest, wbm::valid};
use clap::{crate_authors, crate_version, Clap};
use futures::StreamExt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    match opts.command {
        SubCommand::Count { file, screen_name } => {
            let contents = BufReader::new(File::open(file)?);
            let re =
                regex::Regex::new(format!("^https://twitter.com/(?i){}", screen_name).as_str())
                    .unwrap();

            let mut count = 0;

            for result in contents.lines() {
                if let Ok(line) = result {
                    if re.is_match(&line) {
                        count += 1;
                    } else {
                        if count > 0 {
                            break;
                        }
                    }
                }
            }

            println!("{}", count);
        }
    }

    Ok(())
}

#[derive(Clap)]
#[clap(name = "wbmc", version = crate_version!(), author = crate_authors!())]
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
    Count {
        /// The contents file, sorted by URL
        #[clap(short, long)]
        file: String,
        screen_name: String,
    },
}
