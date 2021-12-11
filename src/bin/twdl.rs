use cancel_culture::{
    cli,
    twitter::{store::Store, Client},
};
use clap::Parser;
use egg_mode::user::TwitterUser;
use futures::{stream::LocalBoxStream, StreamExt, TryStreamExt};
use log::info;
use rusqlite::Connection;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose)?;
    let client = Client::from_config_file(&opts.key_file).await?;
    let store = Store::new(Connection::open(&opts.db_file)?);

    match opts.value {
        Some(value) => {
            let users = client
                .lookup_users(value.split_whitespace().map(|v| v.to_string()))
                .try_collect::<Vec<_>>()
                .await?;

            add_user_follows(&client, &store, &users).await?;
            store.add_users(&users)?;
        }
        None => loop {
            let next = store
                .get_next_users(500)?
                .into_iter()
                .skip(300)
                .take(100)
                .collect::<Vec<_>>();

            let users = client
                .lookup_users(next)
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .filter(|user| user.followers_count < 30000 && !user.protected)
                .take(3)
                .collect::<Vec<_>>();

            add_user_follows(&client, &store, &users).await?;
            store.add_users(&users)?;
        },
    };

    log::logger().flush();

    Ok(())
}

async fn add_user_follows(client: &Client, store: &Store, users: &[TwitterUser]) -> Void {
    type PairResult = egg_mode::error::Result<(u64, u64)>;
    let mut streams: Vec<LocalBoxStream<PairResult>> = Vec::with_capacity(users.len() * 2);

    for user in users {
        info!("Adding streams for: {}", user.screen_name);
        streams.push(
            client
                .follower_ids(user.id)
                .map_ok(move |id| (id, user.id))
                .boxed_local(),
        );
        streams.push(
            client
                .followed_ids(user.id)
                .map_ok(move |id| (user.id, id))
                .boxed_local(),
        );
    }

    let relations = futures::stream::select_all(streams)
        .try_collect::<Vec<(u64, u64)>>()
        .await?;

    Ok(store.add_follows(relations)?)
}

#[derive(Parser)]
#[clap(name = "stores", version, author)]
struct Opts {
    /// TOML file containing Twitter API keys
    #[clap(short, long, default_value = "keys.toml")]
    key_file: String,
    /// SQLite database file
    #[clap(short, long, default_value = "twitter.db")]
    db_file: String,
    /// Level of verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,
    value: Option<String>,
}
