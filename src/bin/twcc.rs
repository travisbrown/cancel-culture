use cancel_culture::{cli, reports::deleted_tweets::DeletedTweetReport, wbm};
use clap::Parser;
use egg_mode::{tweet::Tweet, user::TwitterUser};
use egg_mode_extras::{client::TokenType, util::extract_status_id};
use futures::TryStreamExt;
use itertools::Itertools;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;

const CDX_PAGE_LIMIT: usize = 150000;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Twitter API client error")]
    EggMode(#[from] egg_mode::error::Error),
    #[error("Twitter API client extensions error")]
    EggModeExtras(#[from] egg_mode_extras::error::Error),
    #[error("Failure to read from standard input")]
    Stdin(#[source] std::io::Error),
    #[error("The tweet ID {0}, which was supposed to be a reply, was not a reply")]
    NotReply(u64),
    #[error("Failure to read from CDX JSON file: {0}")]
    CdxJson(#[source] std::io::Error),
    #[error("Failure occurred when parsing a tweet id string: {0}")]
    TweetIdParse(String),
    #[error("Error occurred in the http client: {0}")]
    HttpClient(#[from] reqwest::Error),
    #[error("Wayback Machine CDX client error")]
    WaybackCdx(#[from] wayback_rs::cdx::Error),
    #[error("Wayback Machine download client error")]
    WaybackDownloader(#[from] wayback_rs::downloader::Error),
    #[error("Wayback Machine store error")]
    WbmStoreError(#[from] wbm::store::Error),
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose).unwrap();

    let client = egg_mode_extras::Client::from_config_file(&opts.key_file).await?;

    match opts.command {
        SubCommand::ListFollowers {
            ids_only,
            screen_name,
            user_token,
        } => {
            let token_type = if user_token {
                egg_mode_extras::client::TokenType::User
            } else {
                egg_mode_extras::client::TokenType::App
            };

            let client = egg_mode_extras::Client::from_config_file(&opts.key_file).await?;
            let stream = screen_name
                .map(|name| client.follower_ids(name, token_type))
                .unwrap_or_else(|| client.self_follower_ids());

            if ids_only {
                stream
                    .try_for_each(|id| async move {
                        println!("{}", id);
                        Ok(())
                    })
                    .await?;
            } else {
                let ids = stream.try_collect::<Vec<_>>().await?;
                let users = client
                    .lookup_users(ids, token_type)
                    .try_collect::<Vec<_>>()
                    .await?;
                print_user_report(&users);
            }
            Ok(())
        }
        SubCommand::ListFriends {
            ids_only,
            screen_name,
            user_token,
        } => {
            let token_type = if user_token {
                egg_mode_extras::client::TokenType::User
            } else {
                egg_mode_extras::client::TokenType::App
            };

            let client = egg_mode_extras::Client::from_config_file(&opts.key_file).await?;
            let stream = screen_name
                .map(|name| client.followed_ids(name, token_type))
                .unwrap_or_else(|| client.self_followed_ids());

            if ids_only {
                stream
                    .try_for_each(|id| async move {
                        println!("{}", id);
                        Ok(())
                    })
                    .await?;
            } else {
                let ids = stream.try_collect::<Vec<_>>().await?;
                let users = client
                    .lookup_users(ids, token_type)
                    .try_collect::<Vec<_>>()
                    .await?;
                print_user_report(&users);
            }
            Ok(())
        }
        SubCommand::ListBlocks { ids_only } => {
            let ids: Vec<u64> = client.blocked_ids().try_collect::<Vec<_>>().await?;
            if ids_only {
                for id in ids {
                    println!("{}", id);
                }
            } else {
                let users = client
                    .lookup_users(ids, TokenType::App)
                    .try_collect::<Vec<_>>()
                    .await?;
                print_user_report(&users);
            }
            Ok(())
        }
        SubCommand::ListUnmutuals => {
            let follower_ids: HashSet<u64> = client
                .self_follower_ids()
                .try_collect::<HashSet<_>>()
                .await?;
            let followed_ids: HashSet<u64> = client
                .self_followed_ids()
                .try_collect::<HashSet<_>>()
                .await?;

            let ids = follower_ids
                .symmetric_difference(&followed_ids)
                .cloned()
                .collect::<Vec<_>>();
            log::info!("Looking up {} users", ids.len());

            let mut users = client
                .lookup_users(ids, TokenType::App)
                .try_collect::<Vec<_>>()
                .await?;
            users.sort_by_key(|user| -user.followers_count);

            for user in users {
                if follower_ids.contains(&user.id) {
                    print!("<");
                } else {
                    print!(">");
                }
                println!(" {:16}{:>9}", user.screen_name, user.followers_count);
            }
            Ok(())
        }
        SubCommand::ImportBlocks => {
            let stdin = std::io::stdin();
            let mut buffer = String::new();
            let mut handle = stdin.lock();
            handle.read_to_string(&mut buffer).map_err(Error::Stdin)?;

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
        SubCommand::ListTweets {
            retweets,
            media,
            withheld,
            screen_name,
        } => client
            .user_tweets(screen_name, true, retweets, TokenType::App)
            .try_for_each(|tweet| async move {
                println!(
                    "{}",
                    tweet_to_report(&tweet, retweets, media, withheld, false)
                );
                Ok(())
            })
            .await
            .map_err(Error::from),
        SubCommand::LookupTweets {
            retweets,
            media,
            withheld,
        } => {
            let stdin = std::io::stdin();
            let mut buffer = String::new();
            let mut handle = stdin.lock();
            handle.read_to_string(&mut buffer).map_err(Error::Stdin)?;

            let ids = buffer
                .split_whitespace()
                .flat_map(|input| input.parse::<u64>().ok());

            let comma_count = vec![retweets, media, withheld]
                .iter()
                .filter(|v| **v)
                .count();

            client
                .lookup_tweets(ids, TokenType::App)
                .try_for_each(|(id, result)| async move {
                    match result {
                        Some(tweet) => {
                            println!(
                                "{}",
                                tweet_to_report(&tweet, retweets, media, withheld, true)
                            );
                        }
                        None => {
                            println!("{},0{}", id, ",".repeat(comma_count));
                        }
                    }

                    Ok(())
                })
                .await
                .map_err(Error::from)
        }
        SubCommand::LookupReply { query } => {
            let reply_id = extract_status_id(&query).ok_or_else(|| Error::TweetIdParse(query))?;
            match client.lookup_reply_parent(reply_id, TokenType::App).await? {
                Some((user, id)) => {
                    println!("https://twitter.com/{}/status/{}", user, id);
                    Ok(())
                }
                None => Err(Error::NotReply(reply_id)),
            }
        }
        SubCommand::BlockedFollows { screen_name } => {
            let blocks = client.blocked_ids().try_collect::<HashSet<u64>>().await?;
            let blocked_friends = client
                .followed_ids(screen_name.clone(), TokenType::App)
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .filter(|id| blocks.contains(id))
                .collect::<Vec<_>>();

            if blocked_friends.is_empty() {
                eprintln!("{} does not follow anyone you've blocked", screen_name);
            } else {
                let mut blocked_follows = client
                    .lookup_users(blocked_friends, TokenType::App)
                    .try_collect::<Vec<_>>()
                    .await?;
                blocked_follows.sort_by_key(|u| -u.followers_count);

                for user in blocked_follows {
                    println!("@{:16}{:>9}", user.screen_name, user.followers_count);
                }
            }

            Ok(())
        }
        SubCommand::FollowerReport { screen_name } => {
            let blocks = client.blocked_ids().try_collect::<HashSet<u64>>().await?;
            let their_followers = client
                .follower_ids(screen_name.clone(), TokenType::App)
                .try_collect::<HashSet<u64>>()
                .await?;

            let your_followers = client
                .self_follower_ids()
                .try_collect::<HashSet<u64>>()
                .await?;

            let your_followeds = client
                .self_followed_ids()
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
            let mut common_users = client
                .lookup_users(common, TokenType::App)
                .try_collect::<Vec<_>>()
                .await?;

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
            handle.read_to_string(&mut buffer).map_err(Error::Stdin)?;

            let ids = buffer
                .split_whitespace()
                .flat_map(|input| input.parse::<u64>().ok());

            client
                .lookup_tweets(ids, TokenType::App)
                .try_for_each(|(id, tweet)| async move {
                    println!("{},{}", id, if tweet.is_some() { "1" } else { "0" });
                    Ok(())
                })
                .await?;

            Ok(())
        }
        SubCommand::DeletedTweets {
            limit,
            report,
            include_failed,
            ref store,
            ref cdx,
            ref screen_name,
        } => {
            let index_client = wayback_rs::cdx::IndexClient::default();
            let downloader = wayback_rs::Downloader::default();
            let mut items = match cdx {
                Some(cdx_path) => {
                    let cdx_file = File::open(cdx_path).map_err(Error::CdxJson)?;
                    wayback_rs::cdx::IndexClient::load_json(cdx_file)?
                }
                None => {
                    let url = format!("twitter.com/{}/status/*", screen_name);
                    index_client
                        .stream_search(&url, CDX_PAGE_LIMIT)
                        .try_collect::<Vec<_>>()
                        .await?
                }
            };

            items.sort_unstable_by_key(|item| item.url.clone());

            let results = items.into_iter().group_by(|item| item.url.clone());

            let store = match store {
                Some(dir) => Some(wbm::store::Store::load(dir)?),
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
                        let last = valid.iter().map(|item| item.archived_at).max();
                        let first = valid.into_iter().min_by_key(|item| item.archived_at);

                        first.zip(last).map(|(f, l)| (id, l, f))
                    })
                })
                .collect::<Vec<_>>();

            candidates.sort_unstable_by_key(|(_, last, _)| *last);
            candidates.reverse();

            let selected = candidates.into_iter().take(limit.unwrap_or(usize::MAX));

            let mut by_id: HashMap<u64, wayback_rs::Item> = HashMap::new();

            for (id, _, current) in selected {
                match by_id.get(&id) {
                    Some(latest) => {
                        if latest.archived_at < current.archived_at {
                            by_id.insert(id, current);
                        }
                    }
                    None => {
                        by_id.insert(id, current);
                    }
                }
            }

            let deleted_status = client
                .lookup_tweets(by_id.iter().map(|(k, _)| *k), TokenType::App)
                .try_collect::<Vec<_>>()
                .await?;

            let mut deleted = deleted_status
                .into_iter()
                .filter(|(_, v)| v.is_none())
                .collect::<Vec<_>>();

            deleted.sort_by_key(|(k, _)| *k);

            use cancel_culture::browser::twitter::parser::BrowserTweet;

            let mut report_items = HashMap::<u64, (BrowserTweet, wayback_rs::Item)>::new();

            if let Some(s) = store.as_ref() {
                let mut items = Vec::with_capacity(by_id.len());
                for (id, _) in &deleted {
                    if let Some(item) = by_id.get(id) {
                        if s.read(&item.digest).unwrap_or_default().is_none() {
                            items.push(item.clone());
                        }
                    }
                }

                log::info!("Saving {} items to store", items.len());
                s.save_all(&downloader, &items, true, 4).await?;
            }

            let mut empty_items = vec![];

            for (id, _) in deleted {
                if let Some(item) = by_id.get(&id) {
                    if report {
                        if let Some(content) = match store {
                            Some(ref store) => match store.read(&item.digest) {
                                Ok(content) => content,
                                Err(error) => {
                                    log::error!(
                                        "Invalid UTF-8 bytes in item with digest {} and URL {}",
                                        item.digest,
                                        item.url
                                    );
                                    None
                                }
                            },
                            None => {
                                log::info!("Downloading {}", item.url);
                                match downloader.download_item(item).await {
                                    Ok(bytes) => Some(match String::from_utf8_lossy(&bytes) {
                                        Cow::Borrowed(value) => value.to_string(),
                                        Cow::Owned(value_with_replacements) => {
                                            log::error!(
                                            "Invalid UTF-8 bytes in item with digest {} and URL {}",
                                            item.digest,
                                            item.url
                                        );
                                            value_with_replacements
                                        }
                                    }),
                                    Err(_) => {
                                        log::warn!("Unable to download {}", item.url);
                                        None
                                    }
                                }
                            }
                        } {
                            let html = scraper::Html::parse_document(&content);

                            let mut tweets =
                                cancel_culture::browser::twitter::parser::extract_tweets(&html);

                            if tweets.is_empty() {
                                if let Some(tweet) =
                                    cancel_culture::browser::twitter::parser::extract_tweet_json(
                                        &content,
                                    )
                                {
                                    tweets.push(tweet);
                                }
                            }

                            if tweets.is_empty() {
                                empty_items.push(item);
                                log::warn!("Unable to find tweets for {}", item.url);
                            }

                            for tweet in tweets {
                                if tweet.user_screen_name.to_lowercase()
                                    == *screen_name.to_lowercase()
                                {
                                    match report_items.get(&tweet.id) {
                                        Some((saved_tweet, _)) => {
                                            if saved_tweet.text.len() < tweet.text.len() {
                                                report_items
                                                    .insert(tweet.id, (tweet, item.clone()));
                                            }
                                        }
                                        None => {
                                            report_items.insert(tweet.id, (tweet, item.clone()));
                                        }
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
                    .lookup_tweets(report_items_vec.iter().map(|(k, _)| **k), TokenType::App)
                    .map_ok(|(k, v)| (k, v.is_some()))
                    .try_collect::<HashMap<_, _>>()
                    .await?;

                let deleted_count = deleted_status.iter().filter(|(_, v)| !*v).count();
                let undeleted_count = report_items_vec.len() - deleted_count;

                let report = DeletedTweetReport::new(screen_name, deleted_count, undeleted_count);

                println!("{}", report);

                for (id, (tweet, item)) in report_items_vec {
                    let time = tweet.time.format("%e %B %Y");

                    if *deleted_status.get(id).unwrap_or(&false) {
                        println!(
                            "* [{}](https://web.archive.org/web/{}/{}) ([live](https://twitter.com/{}/status/{})): {} <!--{}-->",
                            time,
                            item.timestamp(),
                            item.url,
                            tweet.user_screen_name,
                            tweet.id,
                            escape_tweet_text(&tweet.text),
                            tweet.id
                        );
                    } else {
                        println!(
                            "* [{}](https://web.archive.org/web/{}/{}): {} <!--{}-->",
                            time,
                            item.timestamp(),
                            item.url,
                            escape_tweet_text(&tweet.text),
                            tweet.id
                        );
                    }
                }

                if include_failed && !empty_items.is_empty() {
                    println!("\n{} URLs could not be parsed:\n", empty_items.len());

                    for item in empty_items {
                        println!(
                            "* [{}](https://web.archive.org/web/{}/{})",
                            item.url,
                            item.timestamp(),
                            item.url
                        );
                    }
                }
            }

            log::logger().flush();

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
    text.replace(r"\'", "'").replace('\n', " ")
}

#[derive(Parser)]
#[clap(name = "twcc", version, author)]
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
    /// For a given user, list everyone they follow who you block
    BlockedFollows { screen_name: String },
    /// For a given user, print a report about their followers
    FollowerReport { screen_name: String },
    /// Get the URL of a tweet given the URL or status ID of a reply
    LookupReply { query: String },
    /// Check whether a list of status IDs (from stdin) still exist
    CheckExistence,
    /// List Wayback Machine URLs for all deleted tweets by a user
    DeletedTweets {
        #[clap(short = 'l', long)]
        /// Only check the tweets the Wayback Machine most recently knows about
        limit: Option<usize>,
        /// Print a Markdown report with full text
        #[clap(short = 'r', long)]
        report: bool,
        /// Include a list of URL snapshots that could not be parsed
        #[clap(long)]
        include_failed: bool,
        /// Local store directory for downloaded Wayback files
        #[clap(short = 's', long)]
        store: Option<String>,
        /// Optional JSON file path for CDX results (useful for large accounts)
        #[clap(short = 'c', long)]
        cdx: Option<String>,
        screen_name: String,
    },
    /// Print a list of all users who follow you (or someone else)
    ListFollowers {
        /// Print only the user's ID (by default you get the ID and screen name)
        #[clap(short = 'i', long)]
        ids_only: bool,
        /// The user to list followers of (by default yourself)
        #[clap(short = 'u', long)]
        screen_name: Option<String>,
        /// Use user token instead of app token (won't work for accounts that block you)
        #[clap(long)]
        user_token: bool,
    },
    /// Print a list of all users you (or someone else) follows
    ListFriends {
        /// Print only the user's ID (by default you get the ID and screen name)
        #[clap(short = 'i', long)]
        ids_only: bool,
        /// The user to list friends of (by default yourself)
        #[clap(short = 'u', long)]
        screen_name: Option<String>,
        /// Use user token instead of app token (won't work for accounts that block you)
        #[clap(long)]
        user_token: bool,
    },
    /// Print a list of all users you've blocked
    ListBlocks {
        /// Print only the user's ID (by default you get the ID and screen name)
        #[clap(short = 'i', long)]
        ids_only: bool,
    },
    /// Print a list of (up to approximately 3200) tweet IDs for a user
    ListTweets {
        /// Include retweet information
        #[clap(short = 'r', long)]
        retweets: bool,
        /// Include media information
        #[clap(short = 'm', long)]
        media: bool,
        /// Include withholding codes
        #[clap(short = 'w', long)]
        withheld: bool,
        /// The user whose tweets you want to list
        screen_name: String,
    },
    /// Read tweet IDs from stdin and print info
    LookupTweets {
        /// Include retweet information
        #[clap(short = 'r', long)]
        retweets: bool,
        /// Include media information
        #[clap(short = 'm', long)]
        media: bool,
        /// Include withholding codes
        #[clap(short = 'w', long)]
        withheld: bool,
    },
    /// Block a list of user IDs (from stdin)
    ImportBlocks,
    /// List everyone you follow or who follows you who is not a mutual
    ListUnmutuals,
}

fn tweet_to_report(
    tweet: &Tweet,
    retweets: bool,
    media: bool,
    withheld: bool,
    include_status: bool,
) -> String {
    let id = tweet.id;

    let retweet_info = tweet.retweeted_status.as_ref().map(|retweeted| {
        let user = retweeted.user.as_ref().unwrap();
        (retweeted.id, user.id, &user.screen_name)
    });

    let media_info = tweet
        .extended_entities
        .as_ref()
        .map(|entities| {
            entities
                .media
                .iter()
                .map(|entity| entity.expanded_url.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut result = id.to_string();

    if include_status {
        result.push_str(",1");
    }

    if retweets {
        result.push(',');
        if let Some((id, user_id, screen_name)) = retweet_info {
            result.push_str(&format!("{};{};{}", id, user_id, screen_name));
        }
    }
    if media {
        result.push_str(&format!(",{}", media_info.join(";")));
    }
    if withheld {
        result.push_str(&format!(
            ",{}",
            tweet
                .withheld_in_countries
                .as_ref()
                .map(|codes| codes.join(";"))
                .unwrap_or_default()
        ));
    }

    result
}
