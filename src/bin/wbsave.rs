use cancel_culture::{cli, wayback};
use clap::{crate_authors, crate_version, Clap};
use std::io::BufRead;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose)?;

    let fantoccini_client = cancel_culture::browser::make_client_or_panic(
        &opts.browser,
        !opts.disable_headless,
        opts.host.as_deref(),
        opts.port,
    )
    .await;

    let mut client = wayback::web::Client::new(fantoccini_client);
    client.login(&opts.username, &opts.password).await?;

    let stdin = std::io::stdin();
    let urls = stdin.lock().lines();

    for result in urls {
        let url = result.expect("Invalid input");

        let saved_url = client.save(&url).await?;
        tokio::time::sleep(std::time::Duration::from_millis(10000)).await;

        println!("{}", saved_url);
    }

    log::logger().flush();

    Ok(())
}

#[derive(Clap)]
#[clap(version = crate_version!(), author = crate_authors!())]
struct Opts {
    #[clap(short = 'u', long)]
    username: String,
    #[clap(short = 'x', long)]
    password: String,
    #[clap(short, long)]
    host: Option<String>,
    #[clap(short, long)]
    port: Option<u16>,
    #[clap(short = 'n', long)]
    disable_headless: bool,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    #[clap(short, long, default_value = "chrome")]
    browser: String,
}
