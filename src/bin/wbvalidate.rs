use cancelculture::wayback::Store;
use std::fs::File;
use std::path::Path;

type Void = Result<(), Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Void {
    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stderr,
    );

    let args: Vec<String> = std::env::args().collect();
    let path = Path::new(&args[1]);
    let store = Store::load("wayback")?;

    if path.is_file() {
        let mut file = File::open(path)?;
        let hash = Store::digest_gz(&mut file)?;
        log::info!("{}", hash);
    } else {
        let contents = std::fs::read_dir(path)?;
        let mut count = 0;
        let mut failed = 0;
        let mut failed302 = 0;

        for result in contents {
            let path = result?.path();

            if path.is_file() {
                let mut file = File::open(&path)?;
                let hash = Store::digest_gz(&mut file)?;
                let stem = path
                    .file_stem()
                    .and_then(|oss| oss.to_str())
                    .expect("Unexpected filename");
                if hash == stem {
                    count += 1;
                } else {
                    log::error!("Bad file: {} (expected {})", stem, hash);
                    let items = store.items_by_digest(stem).await;

                    if items.iter().filter(|item| item.status == Some(302)).count() > 0 {
                        failed302 += 1;
                    }

                    for item in items {
                        if item.status != Some(302) {
                            log::error!("    {:?}", item);
                        }
                    }

                    failed += 1;
                }
            }
        }

        if failed > 0 {
            log::error!(
                "Failed to validate {} files ({} redirects)",
                failed,
                failed302
            );
        }

        log::info!("Validated {} files", count);
    }

    Ok(())
}
