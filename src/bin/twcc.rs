use cancel_culture::{
    twitter::{extract_status_id, Client, Error, Result},
    wayback,
};
use clap::{crate_authors, crate_version, Clap};
use egg_mode::user::TwitterUser;
use futures::TryStreamExt;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::io::Read;

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    let log_level = match opts.verbose {
        0 => simplelog::LevelFilter::Off,
        1 => simplelog::LevelFilter::Error,
        2 => simplelog::LevelFilter::Warn,
        3 => simplelog::LevelFilter::Info,
        4 => simplelog::LevelFilter::Debug,
        _ => simplelog::LevelFilter::Trace,
    };

    let _ = simplelog::TermLogger::init(
        log_level,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stderr,
    );

    let client = Client::from_config_file(&opts.key_file).await?;

    match opts.command {
        SubCommand::ListFollowers(ListFollowers {
            ids_only,
            screen_name,
        }) => {
            let ids = match screen_name {
                Some(name) => client.follower_ids(name).try_collect::<Vec<_>>().await?,
                None => client.follower_ids_self().try_collect::<Vec<_>>().await?,
            };

            if ids_only {
                for id in ids {
                    println!("{}", id);
                }
            } else {
                let users = client.lookup_users(ids).try_collect::<Vec<_>>().await?;
                print_user_report(&users);
            }
            Ok(())
        }
        SubCommand::ListFriends(ListFriends {
            ids_only,
            screen_name,
        }) => {
            let ids = match screen_name {
                Some(name) => client.followed_ids(name).try_collect::<Vec<_>>().await?,
                None => client.followed_ids_self().try_collect::<Vec<_>>().await?,
            };

            if ids_only {
                for id in ids {
                    println!("{}", id);
                }
            } else {
                let users = client.lookup_users(ids).try_collect::<Vec<_>>().await?;
                print_user_report(&users);
            }
            Ok(())
        }
        SubCommand::ListBlocks(ListBlocks { ids_only }) => {
            let ids = client.blocks().await?;
            if ids_only {
                for id in ids {
                    println!("{}", id);
                }
            } else {
                let users = client.lookup_users(ids).try_collect::<Vec<_>>().await?;
                print_user_report(&users);
            }
            Ok(())
        }
        SubCommand::ListTweets(ListTweets { screen_name }) => {
            let ids = client
                .tweets(screen_name, true, true)
                .map_ok(|status| status.id)
                .try_collect::<Vec<_>>()
                .await?;

            for id in ids {
                println!("{}", id);
            }

            Ok(())
        }
        SubCommand::LookupReply(LookupReply { query }) => {
            let reply_id = Client::parse_tweet_id(&query)?;
            match client.get_in_reply_to(reply_id).await? {
                Some((user, id)) => {
                    println!("https://twitter.com/{}/status/{}", user, id);
                    Ok(())
                }
                None => Err(Error::NotReplyError(reply_id)),
            }
        }
        SubCommand::BlockedFollows(BlockedFollows { screen_name }) => {
            let blocks = client.blocks().await?.into_iter().collect::<HashSet<u64>>();
            let blocked_friends = client
                .followed_ids(screen_name.clone())
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .filter(|id| blocks.contains(id))
                .collect::<Vec<_>>();

            if blocked_friends.is_empty() {
                eprintln!("{} does not follow anyone you've blocked", screen_name);
            } else {
                let mut blocked_follows = client
                    .lookup_users(blocked_friends)
                    .try_collect::<Vec<_>>()
                    .await?;
                blocked_follows.sort_by_key(|u| -u.followers_count);

                for user in blocked_follows {
                    println!("@{:16}{:>9}", user.screen_name, user.followers_count);
                }
            }

            Ok(())
        }
        SubCommand::CheckExistence => {
            let stdin = std::io::stdin();
            let mut buffer = String::new();
            let mut handle = stdin.lock();
            handle.read_to_string(&mut buffer)?;

            let ids = buffer
                .split_whitespace()
                .flat_map(|input| input.parse::<u64>().ok());

            let status_map = client.statuses_exist(ids).await?;
            let mut missing = status_map.into_iter().collect::<Vec<_>>();
            missing.sort_unstable();

            for id in missing {
                println!("{} {}", id.0, id.1);
            }

            Ok(())
        }
        SubCommand::DeletedTweets(DeletedTweets {
            limit,
            ref screen_name,
        }) => {
            let wayback_client = wayback::Client::new();
            let url = format!("twitter.com/{}/status/*", screen_name);
            let mut items = wayback_client.search(&url).await?;

            items.sort_unstable_by_key(|item| item.url.clone());

            let results = items.into_iter().group_by(|item| item.url.clone());

            let mut candidates = results
                .into_iter()
                .flat_map(|(k, vs)| {
                    extract_status_id(&k).and_then(|id| {
                        // We currently exclude redirects here, which represent retweets.
                        let valid = vs
                            .into_iter()
                            .filter(|item| item.status.is_none() || item.status == Some(200))
                            .collect::<Vec<_>>();
                        let last = valid.iter().map(|item| item.archived).max();
                        let first = valid.into_iter().min_by_key(|item| item.archived);

                        first.zip(last).map(|(f, l)| (id, l, f))
                    })
                })
                .collect::<Vec<_>>();

            candidates.sort_unstable_by_key(|(_, last, _)| *last);
            candidates.reverse();

            let selected = candidates.into_iter().take(limit.unwrap_or(usize::MAX));

            let mut by_id: HashMap<u64, wayback::Item> = HashMap::new();

            for (id, _, current) in selected {
                match by_id.get(&id) {
                    Some(latest) => {
                        if latest.archived < current.archived {
                            by_id.insert(id, current);
                        }
                    }
                    None => {
                        by_id.insert(id, current);
                    }
                }
            }

            let deleted_status = client.statuses_exist(by_id.iter().map(|(k, _)| *k)).await?;

            let mut deleted = deleted_status
                .into_iter()
                .filter(|(_, v)| !v)
                .collect::<Vec<_>>();

            deleted.sort_by_key(|(k, _)| *k);

            for (id, _) in deleted {
                if let Some(item) = by_id.get(&id) {
                    println!(
                        "https://web.archive.org/web/{}/{}",
                        item.timestamp(),
                        item.url
                    );
                }
            }

            Ok(())
        }
    }
}

fn print_user_report(users: &[TwitterUser]) {
    for user in users {
        println!("{} {} {}", user.id, user.screen_name, user.followers_count);
    }
}

#[derive(Clap)]
#[clap(name = "twcc", version = crate_version!(), author = crate_authors!())]
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
    #[clap(version = crate_version!(), author = crate_authors!())]
    BlockedFollows(BlockedFollows),
    #[clap(version = crate_version!(), author = crate_authors!())]
    LookupReply(LookupReply),
    /// Checks whether a list of status IDs (from stdin) still exist
    #[clap(version = crate_version!(), author = crate_authors!())]
    CheckExistence,
    #[clap(version = crate_version!(), author = crate_authors!())]
    DeletedTweets(DeletedTweets),
    #[clap(version = crate_version!(), author = crate_authors!())]
    ListFollowers(ListFollowers),
    #[clap(version = crate_version!(), author = crate_authors!())]
    ListFriends(ListFriends),
    #[clap(version = crate_version!(), author = crate_authors!())]
    ListBlocks(ListBlocks),
    #[clap(version = crate_version!(), author = crate_authors!())]
    ListTweets(ListTweets),
}

/// Get the URL of a tweet given the URL or status ID of a reply
#[derive(Clap)]
struct LookupReply {
    query: String,
}

/// For a given user, list everyone they follow who you block
#[derive(Clap)]
struct BlockedFollows {
    screen_name: String,
}

/// Lists Wayback Machine URLs for all deleted tweets by a user
#[derive(Clap)]
struct DeletedTweets {
    #[clap(short = 'l', long)]
    /// Only check the tweets the Wayback Machine most recently knows about
    limit: Option<usize>,
    screen_name: String,
}

/// Print a list of all users who follow you (or someone else)
#[derive(Clap)]
struct ListFollowers {
    /// Print only the user's ID (by default you get the ID and screen name)
    #[clap(short = 'i', long)]
    ids_only: bool,
    /// The user to list followers of (by default yourself)
    #[clap(short = 'u', long)]
    screen_name: Option<String>,
}

/// Print a list of all users you (or someone else) follows
#[derive(Clap)]
struct ListFriends {
    /// Print only the user's ID (by default you get the ID and screen name)
    #[clap(short = 'i', long)]
    ids_only: bool,
    /// The user to list friends of (by default yourself)
    #[clap(short = 'u', long)]
    screen_name: Option<String>,
}

/// Print a list of (up to approximately 3200) tweet IDs for a user
#[derive(Clap)]
struct ListTweets {
    /// The user whose tweets you want to list
    screen_name: String,
}

/// Print a list of all users you've blocked
#[derive(Clap)]
struct ListBlocks {
    /// Print only the user's ID (by default you get the ID and screen name)
    #[clap(short = 'i', long)]
    ids_only: bool,
}
