use cancel_culture::browser;
use clap::Parser;
use image::DynamicImage;
use std::path::PathBuf;
use std::time::Duration;

const LOADING_DELAY: Duration = Duration::from_millis(1500);

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opts: Opts = Opts::parse();

    let mut client = browser::make_client_or_panic(
        &opts.browser,
        !opts.disable_headless,
        opts.host.as_deref(),
        opts.port,
    )
    .await;

    if let Some(status_id) = opts
        .status
        .parse::<u64>()
        .ok()
        .or_else(|| egg_mode_extras::util::extract_status_id(&opts.status))
    {
        let full_name = &format!("{}-full.png", status_id);
        let crop_name = &format!("{}.png", status_id);

        let mut full_path = PathBuf::new();
        let mut crop_path = PathBuf::new();

        if let Some(directory) = opts.directory {
            full_path.push(&directory);
            crop_path.push(&directory);
        }

        full_path.push(full_name);
        crop_path.push(crop_name);

        let img = browser::twitter::shoot_tweet(
            &mut client,
            status_id,
            opts.width,
            opts.height,
            Some(LOADING_DELAY),
        )
        .await?;

        img.save(full_path)
            .map_err(browser::twitter::ScreenshotError::from)?;

        let as_rgba = img.into_rgba8();

        if let Some((x, y, w, h)) = browser::twitter::crop_tweet(&as_rgba) {
            let clipping = DynamicImage::ImageRgba8(as_rgba).crop(x, y, w, h);
            clipping
                .save(crop_path)
                .map_err(browser::twitter::ScreenshotError::from)?;
        } else {
            eprintln!("Unable to crop tweet");
        }

        Ok(())
    } else {
        Err(Error::TweetIdParse(opts.status))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid tweet URL")]
    TweetIdParse(String),
    #[error("Screenshot error")]
    Screenshot(#[from] browser::twitter::ScreenshotError),
}

#[derive(Parser)]
#[clap(version, author)]
struct Opts {
    /// Either a tweet URL or a status ID
    status: String,
    #[clap(short, long)]
    host: Option<String>,
    #[clap(short, long)]
    port: Option<u16>,
    #[clap(short = 'n', long)]
    disable_headless: bool,
    #[clap(short, long)]
    directory: Option<String>,
    #[clap(long, default_value = "800")]
    width: u32,
    #[clap(long, default_value = "4000")]
    height: u32,
    #[clap(short, long, default_value = "chrome")]
    browser: String,
}
