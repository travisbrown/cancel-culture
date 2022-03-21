use cancel_culture::cli;
use chrono::Utc;
use clap::Parser;
use egg_mode::user::UserID;
use egg_mode_extras::{client::TokenType, Client};
use futures::{StreamExt, TryStreamExt};
use itertools::Itertools;
use std::collections::HashSet;
use std::io::BufRead;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose)?;
    let client = Client::from_config_file(&opts.key_file).await?;

    match opts.command {
        SubCommand::TweetIdsByUserId { db } => {
            let stdin = std::io::stdin();
            let handle = stdin.lock();
            let ids = handle
                .lines()
                .map(|line| line.ok().and_then(|input| input.parse::<u64>().ok()))
                .collect::<Option<HashSet<u64>>>()
                .unwrap();

            let store = cancel_culture::wbm::tweet::db::TweetStore::new(db, false)?;

            for id in ids {
                let result = store.tweet_ids_by_user_id(id).await?;
                for tweet_id in result {
                    println!("{},{}", id, tweet_id);
                }
            }
        }
        SubCommand::UserJson { timestamp } => {
            let stdin = std::io::stdin();
            let handle = stdin.lock();
            let ids = handle
                .lines()
                .map(|line| line.ok().and_then(|input| input.parse::<u64>().ok()))
                .collect::<Option<HashSet<u64>>>()
                .unwrap();

            let users = client.lookup_users_json(ids, TokenType::App);
            let timestamp = timestamp.as_ref();

            users
                .try_for_each(|mut user| async move {
                    if let Some(fields) = user.as_object_mut() {
                        if let Some(timestamp_field_name) = timestamp {
                            if let Some(previous_value) = fields.insert(
                                timestamp_field_name.clone(),
                                serde_json::json!(Utc::now().timestamp()),
                            ) {
                                log::warn!(
                                    "Timestamp field collision: \"{}\" was {}",
                                    timestamp_field_name,
                                    previous_value
                                );
                            }
                        }
                    } else {
                        log::warn!("Not a JSON object: {}", user);
                    }

                    println!("{}", user);
                    Ok(())
                })
                .await?
        }
        SubCommand::UserInfo { db, md } => {
            let stdin = std::io::stdin();
            let handle = stdin.lock();
            let ids = handle
                .lines()
                .map(|line| line.ok().and_then(|input| input.parse::<u64>().ok()))
                .collect::<Option<Vec<u64>>>()
                .unwrap();

            let store = cancel_culture::wbm::tweet::db::TweetStore::new(db, false)?;
            let mut results = store.get_users(&ids).await?;

            results.sort();

            if md {
                results.reverse();
                println!("|Twitter ID|Screen name|First seen|Last seen|Tweets archived|");
                println!("|----------|-----------|----------|---------|---------------|");
                for result in &results {
                    println!(
                        "|{}|{}|{}|{}|{}|",
                        result.id,
                        result.screen_name,
                        result.first_seen.format("%Y-%m-%d"),
                        result.last_seen.format("%Y-%m-%d"),
                        result.tweet_count
                    );
                }
                println!("\n|Twitter ID|Display names|");
                println!("|----------|-----------|");

                for (id, group) in &results.into_iter().group_by(|result| result.id) {
                    let mut names = group.flat_map(|result| result.names).collect::<Vec<_>>();
                    names.sort();
                    names.dedup();

                    println!(
                        "|{}|{}|",
                        id,
                        names
                            .iter()
                            .map(|name| name.replace('|', "\\|"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            } else {
                let mut writer = csv::WriterBuilder::new()
                    .flexible(true)
                    .from_writer(std::io::stdout());

                for result in results {
                    let record = vec![
                        result.id.to_string(),
                        result.screen_name,
                        result.first_seen.format("%Y-%m-%d").to_string(),
                        result.last_seen.format("%Y-%m-%d").to_string(),
                        result.tweet_count.to_string(),
                        result
                            .names
                            .iter()
                            .map(|name| name.replace(';', "\\;"))
                            .collect::<Vec<_>>()
                            .join(";"),
                    ];

                    writer.write_record(record)?;
                }
            }
        }
        SubCommand::ScreenNames {
            include_screen_name,
        } => {
            let stdin = std::io::stdin();
            let handle = stdin.lock();
            let ids = handle
                .lines()
                .map(|line| line.ok().and_then(|input| input.parse::<u64>().ok()))
                .collect::<Option<Vec<u64>>>()
                .unwrap();
            let mut missing = ids.iter().cloned().collect::<HashSet<_>>();
            let results = client.lookup_users(ids, TokenType::App);

            let valid = results
                .filter_map(|res| async move {
                    match res {
                        Err(error) => {
                            log::error!("Unknown error: {:?}", error);
                            None
                        }
                        Ok(user) => {
                            let withheld_info = user
                                .withheld_in_countries
                                .map(|values| values.join(";"))
                                .unwrap_or_default();
                            log::warn!("{:?}", user.created_at);

                            println!(
                                "{},{}{},{},{},{},{},{}",
                                user.id,
                                if include_screen_name {
                                    format!("{},", user.screen_name)
                                } else {
                                    "".to_string()
                                },
                                if user.verified { 1 } else { 0 },
                                if user.protected { 1 } else { 0 },
                                user.statuses_count,
                                user.followers_count,
                                user.friends_count,
                                withheld_info
                            );
                            Some(user.id)
                        }
                    }
                })
                .collect::<Vec<u64>>()
                .await;

            log::info!("Processing missing users");

            for id in valid {
                missing.remove(&id);
            }

            let mut missing1 = missing.into_iter().collect::<Vec<_>>();
            missing1.sort_unstable();
            let mut missing2 = missing1.split_off(missing1.len() / 2);
            missing2.reverse();

            futures::stream::select(
                client.lookup_users_or_status(missing1, TokenType::App),
                client.lookup_users_or_status(missing2, TokenType::User),
            )
            .try_for_each(|res| async move {
                if let Err((UserID::ID(id), status)) = res {
                    println!("{:?},{}", id, status.code());
                };
                Ok(())
            })
            .await?;
        }
    };

    log::logger().flush();

    Ok(())
}

#[derive(Parser)]
#[clap(name = "twcli", version, author)]
struct Opts {
    /// TOML file containing Twitter API keys
    #[clap(short, long, default_value = "keys.toml")]
    key_file: String,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    #[clap(subcommand)]
    command: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    ScreenNames {
        #[clap(long)]
        include_screen_name: bool,
    },
    UserInfo {
        #[clap(long)]
        db: String,
        #[clap(long)]
        md: bool,
    },
    TweetIdsByUserId {
        #[clap(long)]
        db: String,
    },
    UserJson {
        /// Timestamp field name to add to Twitter JSON object
        #[clap(short, long)]
        timestamp: Option<String>,
    },
}
