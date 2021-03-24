use cancel_culture::{
    cli,
    twitter::{store::Store, Client},
};
use clap::{crate_authors, crate_version, Clap};
use egg_mode::error::{Error, TwitterErrorCode, TwitterErrors};
use egg_mode::user::{TwitterUser, UserID};
use futures::{future, stream::LocalBoxStream, StreamExt, TryStreamExt};
use log::info;
use std::collections::HashSet;
use std::io::BufRead;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let opts: Opts = Opts::parse();
    let _ = cli::init_logging(opts.verbose);
    let client = Client::from_config_file(&opts.key_file).await?;

    match opts.command {
        SubCommand::ScreenNames => {
            let stdin = std::io::stdin();
            let mut handle = stdin.lock();
            let ids = handle
                .lines()
                .map(|line| line.ok().and_then(|input| input.parse::<u64>().ok()))
                .collect::<Option<Vec<u64>>>()
                .unwrap();
            let mut missing = ids.iter().cloned().collect::<HashSet<_>>();
            let results = client.lookup_users(ids);

            let mut valid = results
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

            for id in valid {
                missing.remove(&id);
            }

            client
                .show_users(missing)
                .try_for_each(|res| async {
                    res.map_err(|(user_id, code)| {
                        if let UserID::ID(id) = user_id {
                            println!("{:?},{}", id, code);
                        }
                    });
                    Ok(())
                })
                .await?;
        }
    };

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
}
