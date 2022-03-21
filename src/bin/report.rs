use cancel_culture::browser::twitter::parser::BrowserTweet;
use cancel_culture::{cli, wbm};
use clap::Parser;
use csv::ReaderBuilder;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use wayback_rs::Item;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);

    match opts.command {
        SubCommand::All { db, items } => {
            let status_ids = cli::read_stdin()?
                .lines()
                .map(|line| line.trim().parse::<u64>())
                .collect::<Result<Vec<_>, _>>()?;

            let tweet_store = wbm::tweet::db::TweetStore::new(db, false)?;

            let tweets = tweet_store.get_multi_tweets(&status_ids).await?;

            log::info!("{} tweets loaded", tweets.len());

            let hashes = tweets
                .iter()
                .map(|(_, digest)| digest.clone())
                .collect::<HashSet<String>>();

            let mut reader = ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .from_reader(File::open(items)?);

            let by_digest = reader
                .records()
                .filter_map(|result| {
                    let row = result.unwrap();

                    if row.len() > 2 && hashes.contains(&row[2]) {
                        Some((
                            row[2].to_string(),
                            Item::parse_optional_record(
                                row.get(0),
                                row.get(1),
                                row.get(2),
                                row.get(3),
                                if row.len() == 5 {
                                    Some("0")
                                } else {
                                    row.get(4)
                                },
                                if row.len() == 5 {
                                    row.get(4)
                                } else {
                                    row.get(5)
                                },
                            )
                            .unwrap(),
                        ))
                    } else {
                        None
                    }
                })
                .collect::<HashMap<String, Item>>();

            log::info!("{} items found", by_digest.len());

            let mut result = tweets
                .into_iter()
                .group_by(|(t, _)| t.id)
                .into_iter()
                .map(|(k, v)| (k, v.collect::<Vec<(BrowserTweet, String)>>()))
                .collect::<Vec<_>>();
            result.reverse();

            for (_, versions) in &result {
                let content = &versions
                    .iter()
                    .max_by_key(|(t, _)| t.text.len())
                    .unwrap()
                    .0
                    .text
                    .clone();

                let mut items = vec![];

                for (tweet, digest) in versions {
                    match by_digest.get(digest) {
                        Some(item) => {
                            items.push((tweet, item));
                        }
                        None => {
                            log::error!("Can't find digest: {}", digest);
                        }
                    }
                }

                items.sort_by_key(|(_, item)| item.archived_at);
                items.reverse();

                println!(
                    "#### {} ({})\n\n> {}\n\n",
                    versions[0].0.time.format("%e %B %Y"),
                    versions[0].0.id,
                    content.split('\n').join("\n> ")
                );

                for (tweet, item) in items {
                    println!(
                        "* Archived as @{} on [{}]({})",
                        tweet.user_screen_name,
                        item.archived_at.format("%e %B %Y"),
                        item.wayback_url(false)
                    );
                }
                println!();
            }
        }
    }

    Ok(())
}

#[derive(Parser)]
#[clap(name = "report", version, author)]
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
    All {
        /// The database path
        #[clap(short, long)]
        db: String,
        /// The items file path
        #[clap(short, long)]
        items: String,
    },
}
