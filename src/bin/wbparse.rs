use cancel_culture::browser::twitter::parser::{extract_description, extract_tweets, parse_gz};
use cancel_culture::wayback::Store;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stderr,
    );

    let args: Vec<String> = std::env::args().collect();
    let path = Path::new(&args[1]);
    let store = Store::load("wayback")?;

    if path.is_file() {
        let file = File::open(&path)?;
        let mut gz = GzDecoder::new(file);
        let mut contents = String::new();

        gz.read_to_string(&mut contents)?;

        println!("{}", contents);
    } else {
        let contents = std::fs::read_dir(path)?;
        let mut count = 0;
        let mut failed = 0;

        for result in contents {
            let path = result?.path();

            if path.is_file() {
                let digest = Store::extract_digest(&path).unwrap();
                let mut file = File::open(&path)?;

                let html = parse_gz(&mut file)?;

                let description = extract_description(&html).is_some();
                let tweets = extract_tweets(&html);

                let items = store.items_by_digest(&digest).await;
                let all_html = items.iter().all(|item| item.mimetype == "text/html");

                println!(
                    "{} {} {}",
                    digest,
                    if all_html {
                        if description {
                            "1"
                        } else {
                            "0"
                        }
                    } else {
                        "-"
                    },
                    tweets.len()
                );

                count += 1;

                if tweets.is_empty() && all_html {
                    failed += 1;
                }
            }
        }

        if failed > 0 {
            log::error!("Failed to validate {} files", failed,);
        }

        log::info!("Parsed {} files", count);
    }

    Ok(())
}
