use cancel_culture::{browser::twitter::parser, cli};
use clap::Parser;
use std::fs::File;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    match opts.command {
        SubCommand::Parse { path } => {
            let mut file = File::open(path.clone())?;
            let html = if path.ends_with(".gz") {
                parser::parse_html_gz(&mut file)
            } else {
                parser::parse_html(&mut file)
            };

            let mut out = csv::WriterBuilder::new().from_writer(std::io::stdout());

            for (a, b, c, d, e, f) in parser::extract_phcs(&html?) {
                out.write_record(&[a, b, c, d, e, f.unwrap_or_else(|| "".to_string())])?;
            }
        }
    }

    Ok(())
}

#[derive(Parser)]
#[clap(name = "details", version, author)]
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

#[derive(Parser)]
enum SubCommand {
    Parse {
        /// The file path
        #[clap(short, long)]
        path: String,
    },
}
