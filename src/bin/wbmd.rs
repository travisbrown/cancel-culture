use cancel_culture::{cli, wbm, wbm::valid};
use clap::Parser;
use futures::StreamExt;
use std::path::Path;
use wayback_rs::digest;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose)?;

    match opts.command {
        SubCommand::Create { dir } => {
            valid::ValidStore::create(dir)?;
        }
        SubCommand::Extract { dir, digest } => {
            let store = valid::ValidStore::new(dir);
            if let Some(result) = store.extract(&digest) {
                println!("{}", result?);
            }
        }
        SubCommand::List { dir, prefix } => {
            let store = valid::ValidStore::new(dir);
            let paths = store.paths_for_prefix(&prefix.unwrap_or_else(|| "".to_string()));

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
        SubCommand::DigestsRaw { dir } => {
            for result in std::fs::read_dir(dir)? {
                let entry = result?;

                if entry.path().is_file() {
                    if let Some(name) = entry.path().file_stem().and_then(|os| os.to_str()) {
                        let mut file = std::fs::File::open(entry.path())?;
                        match digest::compute_digest_gz(&mut file) {
                            Ok(digest) => {
                                println!("{},{}", name, digest);
                            }
                            Err(error) => {
                                log::error!("Error at {}: {:?}", name, error);
                            }
                        }
                    } else {
                        log::info!("Ignoring file: {:?}", entry.path());
                    }
                } else {
                    log::info!("Ignoring directory: {:?}", entry.path());
                }
            }
        }
        SubCommand::RenameRaw { dir, out } => {
            let out_path = Path::new(&out);

            for result in std::fs::read_dir(dir)? {
                let entry = result?;

                if entry.path().is_file() {
                    if let Some(name) = entry.path().file_stem().and_then(|os| os.to_str()) {
                        let mut file = std::fs::File::open(entry.path())?;
                        match digest::compute_digest_gz(&mut file) {
                            Ok(digest) => {
                                println!("{},{}", name, digest);
                                std::fs::copy(
                                    entry.path(),
                                    out_path.join(format!("{}.gz", digest)),
                                )?;
                            }
                            Err(error) => {
                                log::error!("Error at {}: {:?}", name, error);
                            }
                        }
                    } else {
                        log::info!("Ignoring file: {:?}", entry.path());
                    }
                } else {
                    log::info!("Ignoring directory: {:?}", entry.path());
                }
            }
        }
        SubCommand::AddFile { dir, input } => {
            let store = valid::ValidStore::new(dir);

            match store.check_file_location(&input)? {
                None => log::warn!("File already exists in store: {}", input),
                Some(Ok((name, location))) => {
                    log::info!("Adding file with digest: {}", name);
                    std::fs::copy(&input, &location)?;

                    println!("{},{}", input, location.to_string_lossy());
                }
                Some(Err((expected, actual))) => {
                    log::error!(
                        "File to add has invalid digest (expected: {}; actual: {}): {}",
                        expected,
                        actual,
                        input
                    );
                }
            }
        }
        SubCommand::SaveTweets { db, store } => {
            let tweet_store = wbm::tweet::db::TweetStore::new(db, false)?;
            let valid_store = valid::ValidStore::new(store);

            wbm::tweet::export_tweets(&valid_store, &tweet_store).await?;
        }
        SubCommand::Get { db } => {
            let status_ids = cli::read_stdin()?
                .lines()
                .map(|line| line.parse::<u64>())
                .collect::<Result<Vec<_>, _>>()?;

            let tweet_store = wbm::tweet::db::TweetStore::new(db, false)?;
            let mut results = tweet_store.get_tweet(&status_ids).await?;
            results.sort_by_key(|(tweet, _)| (tweet.user_screen_name.to_lowercase(), tweet.id));

            let mut out = csv::WriterBuilder::new().from_writer(std::io::stdout());
            let space_re = regex::Regex::new(r" +").unwrap();

            for (tweet, digest) in results {
                out.write_record(&[
                    tweet.user_screen_name,
                    tweet.id.to_string(),
                    digest,
                    space_re
                        .replace_all(&tweet.text.trim().replace('\n', "\\n"), " ")
                        .to_string(),
                ])?;
            }
        }
        SubCommand::Replies { db } => {
            let users = cli::read_stdin()?
                .lines()
                .map(|line| {
                    let fields = line.split(',').collect::<Vec<_>>();

                    fields[0]
                        .parse::<u64>()
                        .map(|id| (id, fields[1].to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;

            let tweet_store = wbm::tweet::db::TweetStore::new(db, false)?;

            for (user_twitter_id, screen_name) in users {
                let results = tweet_store
                    .get_replies(user_twitter_id, &screen_name)
                    .await?;

                for (twitter_id, reply_twitter_id, reply_user_twitter_id, reply_screen_name) in
                    results
                {
                    println!(
                        "{},{},{},{},{},{}",
                        screen_name,
                        reply_screen_name,
                        user_twitter_id,
                        reply_user_twitter_id,
                        twitter_id,
                        reply_twitter_id
                    );
                }
            }
        }
        SubCommand::Interactions { db } => {
            let users = cli::read_stdin()?
                .lines()
                .map(|line| line.parse::<u64>())
                .collect::<Result<Vec<_>, _>>()?;

            let tweet_store = wbm::tweet::db::TweetStore::new(db, false)?;

            for user_twitter_id in users {
                tweet_store
                    .for_each_interaction(
                        user_twitter_id,
                        |(twitter_id, twitter_ts, user_twitter_id, screen_name),
                         (
                            reply_twitter_id,
                            reply_twitter_ts,
                            reply_user_twitter_id,
                            reply_screen_name,
                        )| {
                            println!(
                                "{},{},{},{},{},{}",
                                twitter_id,
                                twitter_ts,
                                user_twitter_id,
                                reply_twitter_id,
                                reply_twitter_ts,
                                reply_user_twitter_id,
                            );
                        },
                    )
                    .await?;
            }
        }
        SubCommand::ScreenNames { db } => {
            let users = cli::read_stdin()?
                .lines()
                .map(|line| line.parse::<u64>())
                .collect::<Result<Vec<_>, _>>()?;

            let tweet_store = wbm::tweet::db::TweetStore::new(db, false)?;

            let result = tweet_store.get_most_common_screen_names(&users).await?;

            let mut pairs: Vec<_> = result.iter().collect();
            pairs.sort();

            for (id, screen_name) in pairs {
                match screen_name {
                    Some(v) => {
                        println!("{},{}", id, v);
                    }
                    None => {
                        log::error!("Unknown ID: {}", id);
                    }
                }
            }
        }
    }

    log::logger().flush();

    Ok(())
}

#[derive(Parser)]
#[clap(name = "wbmd", version, author)]
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
    Create {
        /// The base directory
        #[clap(short, long)]
        dir: String,
    },
    Extract {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        // Digest
        digest: String,
    },
    List {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        /// Optional prefix
        #[clap(short, long)]
        prefix: Option<String>,
    },
    Digests {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        /// Optional prefix
        #[clap(short, long)]
        prefix: Option<String>,
    },
    /// Compute all digests for files in a directory
    DigestsRaw {
        /// The directory
        #[clap(short, long)]
        dir: String,
    },
    /// Compute all digests for files in a directory and rename them accordingly
    RenameRaw {
        /// The directory
        #[clap(short, long)]
        dir: String,
        /// The output directory
        #[clap(short, long)]
        out: String,
    },
    AddFile {
        /// The base directory
        #[clap(short, long)]
        dir: String,
        /// The file path to consider adding
        #[clap(short, long)]
        input: String,
    },
    SaveTweets {
        /// The database file
        #[clap(short, long)]
        db: String,
        /// The base directory
        #[clap(short, long)]
        store: String,
    },
    Get {
        /// The database file
        #[clap(short, long)]
        db: String,
    },
    Replies {
        /// The database file
        #[clap(short, long)]
        db: String,
    },
    Interactions {
        /// The database file
        #[clap(short, long)]
        db: String,
    },
    ScreenNames {
        /// The database file
        #[clap(short, long)]
        db: String,
    },
}
