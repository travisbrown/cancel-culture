use futures::TryStreamExt;
use reqwest::{Client, StatusCode};
use std::time::Duration;
use tokio::time::sleep;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stderr,
    );

    let args: Vec<String> = std::env::args().collect();
    let screen_name = &args[1];

    let twitter_client = cancelculture::twitter::Client::from_config_file("keys.toml").await?;
    let client = Client::new();

    let tweets = twitter_client
        .tweets(screen_name.to_owned(), true, true)
        .map_ok(move |status| {
            (
                status.id,
                format!("https://twitter.com/{}/status/{}", screen_name, status.id),
            )
        })
        .try_collect::<Vec<_>>()
        .await?;

    for (id, url) in tweets.into_iter().skip(30) {
        log::info!("Saving: {}", url);
        let data = [("url", url)];
        let response = client
            .post("https://web.archive.org/save")
            .form(&data)
            .header("Referer", "https://web.archive.org/save")
            .send()
            .await?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await?;

        if status == StatusCode::TOO_MANY_REQUESTS
            || body.contains("reached the limit of active sessions")
        {
            println!("Headers: {:?}", headers);
            sleep(Duration::from_secs(300)).await;
        }
    }

    Ok(())
}
