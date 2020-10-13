use cancelculture::wayback::{Client, Store};

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stderr,
    );

    let args: Vec<String> = std::env::args().collect();

    let client = Client::new();
    let store = Store::load("wayback")?;
    let items = client
        .search(&args[1])
        .await?
        .into_iter()
        .filter(|item| item.url.len() < 80)
        .collect::<Vec<_>>();
    log::info!("{} items to download", items.len());

    client.save_all(&store, &items, 4).await?;

    Ok(())
}
