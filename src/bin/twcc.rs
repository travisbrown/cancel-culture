use cancel_culture::{
    cli,
    reports::deleted_tweets::DeletedTweetReport,
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
    let _ = cli::init_logging(opts.verbose);

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
        SubCommand::ImportBlocks => {
            let stdin = std::io::stdin();
            let mut buffer = String::new();
            let mut handle = stdin.lock();
            handle.read_to_string(&mut buffer)?;

            let ids = buffer
                .split_whitespace()
                .flat_map(|input| input.parse::<u64>().ok())
                .collect::<Vec<_>>();

            for chunk in ids.chunks(128) {
                for id in chunk {
                    log::info!("Blocking user ID: {}", id);
                }

                let res =
                    futures::future::try_join_all(chunk.iter().map(|id| client.block_user(*id)))
                        .await?;

                for user in res {
                    log::warn!("Blocked user: {:12} {}", user.id, user.screen_name);
                }
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
        SubCommand::FollowerReport(FollowerReport { screen_name }) => {
            let blocks = client.blocks().await?.into_iter().collect::<HashSet<u64>>();
            let their_followers = client
                .follower_ids(screen_name.clone())
                .try_collect::<HashSet<u64>>()
                .await?;

            let your_followers = client
                .follower_ids_self()
                .try_collect::<HashSet<u64>>()
                .await?;

            let your_followeds = client
                .followed_ids_self()
                .try_collect::<HashSet<u64>>()
                .await?;

            let blocked_followers = blocks
                .intersection(&their_followers)
                .cloned()
                .collect::<HashSet<u64>>();
            let shared_followers = your_followers
                .intersection(&their_followers)
                .cloned()
                .collect::<HashSet<u64>>();
            let followed_followers = your_followeds
                .intersection(&their_followers)
                .cloned()
                .collect::<HashSet<u64>>();

            let common = blocked_followers
                .union(&shared_followers)
                .cloned()
                .collect::<HashSet<u64>>()
                .union(&followed_followers)
                .cloned()
                .collect::<HashSet<u64>>();
            let mut common_users = client.lookup_users(common).try_collect::<Vec<_>>().await?;

            common_users.sort_by_key(|user| user.id);

            println!("{} has {} followers", screen_name, their_followers.len());
            println!(
                "{} has {} followers who follow you",
                screen_name,
                shared_followers.len()
            );

            for user in &common_users {
                if shared_followers.contains(&user.id) {
                    println!("  {:20} {}", user.id, user.screen_name);
                }
            }

            println!(
                "{} has {} followers you follow",
                screen_name,
                followed_followers.len()
            );

            for user in &common_users {
                if followed_followers.contains(&user.id) {
                    println!("  {:20} {}", user.id, user.screen_name);
                }
            }

            println!(
                "{} has {} followers you've blocked",
                screen_name,
                blocked_followers.len()
            );

            for user in common_users {
                if blocked_followers.contains(&user.id) {
                    println!("  {:20} {}", user.id, user.screen_name);
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
            report,
            ref store,
            ref screen_name,
        }) => {
            let wayback_client = wayback::cdx::Client::new();
            let url = format!("twitter.com/{}/status/*", screen_name);
            let mut items = wayback_client.search(&url).await?;

            items.sort_unstable_by_key(|item| item.url.clone());

            let results = items.into_iter().group_by(|item| item.url.clone());

            let store = match store {
                Some(dir) => Some(wayback::Store::load(dir)?),
                None => None,
            };

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

            use cancel_culture::browser::twitter::parser::BrowserTweet;

            let mut report_items = HashMap::<u64, (BrowserTweet, wayback::Item)>::new();

            if let Some(s) = store.as_ref() {
                let mut items = Vec::with_capacity(by_id.len());
                for (id, _) in &deleted {
                    if let Some(item) = by_id.get(&id) {
                        if s.read(&item.digest).unwrap_or_default().is_none() {
                            items.push(item.clone());
                        }
                    }
                }

                log::info!("Saving {} items to store", items.len());
                wayback_client.save_all(s, &items, 4).await?;
            }

            for (id, _) in deleted {
                if let Some(item) = by_id.get(&id) {
                    if report {
                        let from_store = store
                            .as_ref()
                            .and_then(|s| s.read(&item.digest).unwrap_or_default());
                        let content = match from_store {
                            Some(v) => v,
                            None => {
                                log::info!("Downloading {}", item.url);
                                let bytes = wayback_client.download(item, true).await?;
                                std::str::from_utf8(&bytes)
                                    .expect("Invalid UTF-8 bytes")
                                    .to_string()
                            }
                        };

                        let html = scraper::Html::parse_document(&content);

                        let mut tweets =
                            cancel_culture::browser::twitter::parser::extract_tweets(&html);

                        if tweets.is_empty() {
                            if let Some(tweet) =
                                cancel_culture::browser::twitter::parser::extract_tweets_json(
                                    &content,
                                )
                            {
                                tweets.push(tweet);
                            }
                        }

                        if tweets.is_empty() {
                            log::warn!("Unable to find tweets for {}", item.url);
                        }

                        for tweet in tweets {
                            if tweet.user_screen_name == *screen_name {
                                match report_items.get(&tweet.id) {
                                    Some((saved_tweet, _)) => {
                                        if saved_tweet.text.len() < tweet.text.len() {
                                            report_items.insert(tweet.id, (tweet, item.clone()));
                                        }
                                    }
                                    None => {
                                        report_items.insert(tweet.id, (tweet, item.clone()));
                                    }
                                }
                            }
                        }
                    } else {
                        println!(
                            "https://web.archive.org/web/{}/{}",
                            item.timestamp(),
                            item.url
                        );
                    }
                }
            }

            if report {
                let mut report_items_vec = report_items.iter().collect::<Vec<_>>();
                report_items_vec.sort_unstable_by_key(|(k, _)| -(**k as i64));

                let deleted_status = client
                    .statuses_exist(report_items_vec.iter().map(|(k, _)| **k))
                    .await?;

                let deleted_count = deleted_status.iter().filter(|(_, v)| !(*v)).count();
                let undeleted_count = report_items_vec.len() - deleted_count;

                let report = DeletedTweetReport::new(screen_name, deleted_count, undeleted_count);

                println!("{}", report);

                for (id, (tweet, item)) in report_items_vec {
                    let time = tweet.time.format("%e %B %Y");

                    if *deleted_status.get(id).unwrap_or(&false) {
                        println!(
                            "* [{}](https://web.archive.org/web/{}/{}) ([live](https://twitter.com/{}/status/{})): {}",
                            time,
                            item.timestamp(),
                            item.url,
                            tweet.user_screen_name,
                            tweet.id,
                            escape_tweet_text(&tweet.text)
                        );
                    } else {
                        println!(
                            "* [{}](https://web.archive.org/web/{}/{}): {}",
                            time,
                            item.timestamp(),
                            item.url,
                            escape_tweet_text(&tweet.text)
                        );
                    }
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

fn escape_tweet_text(text: &str) -> String {
    text.replace(r"\'", "'").replace("\n", " ")
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
    FollowerReport(FollowerReport),
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
    /// Blocks a list of user IDs (from stdin)
    #[clap(version = crate_version!(), author = crate_authors!())]
    ImportBlocks,
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

/// For a given user, print a report about their followers
#[derive(Clap)]
struct FollowerReport {
    screen_name: String,
}

/// Lists Wayback Machine URLs for all deleted tweets by a user
#[derive(Clap)]
struct DeletedTweets {
    #[clap(short = 'l', long)]
    /// Only check the tweets the Wayback Machine most recently knows about
    limit: Option<usize>,
    /// Print a Markdown report with full text.
    #[clap(short = 'r', long)]
    report: bool,
    /// Local store directory for downloaded Wayback files.
    #[clap(short = 's', long)]
    store: Option<String>,
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
