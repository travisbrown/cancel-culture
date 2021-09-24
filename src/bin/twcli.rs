use cancel_culture::{cli, twitter::Client};
use clap::{crate_authors, crate_version, Clap};
use egg_mode::user::UserID;
use futures::{StreamExt, TryStreamExt};
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
        SubCommand::UserInfo { db } => {
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
                        .map(|name| name.replace(";", "\\;"))
                        .collect::<Vec<_>>()
                        .join(";"),
                ];

                writer.write_record(record)?;
            }
        }
        SubCommand::ScreenNames => {
            let stdin = std::io::stdin();
            let handle = stdin.lock();
            let ids = handle
                .lines()
                .map(|line| line.ok().and_then(|input| input.parse::<u64>().ok()))
                .collect::<Option<Vec<u64>>>()
                .unwrap();
            let mut missing = ids.iter().cloned().collect::<HashSet<_>>();
            let results = client.lookup_users(ids);

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
                                "{},{},{},{},{},{},{}",
                                user.id,
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
                client.show_users(missing1),
                client.show_users_user_token(missing2),
            )
            .try_for_each(|res| async move {
                if let Err((UserID::ID(id), code)) = res {
                    println!("{:?},{}", id, code);
                };
                Ok(())
            })
            .await?;
        }
    };

    log::logger().flush();

    Ok(())
}

#[derive(Clap)]
#[clap(name = "twcli", version = crate_version!(), author = crate_authors!())]
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

#[derive(Clap)]
enum SubCommand {
    ScreenNames,
    UserInfo {
        #[clap(long)]
        db: String,
    },
    TweetIdsByUserId {
        #[clap(long)]
        db: String,
    },
}
