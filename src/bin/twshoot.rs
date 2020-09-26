use cancelculture::browser;
use cancelculture::twitter;
use clap::{crate_authors, crate_version, Clap};
use image::{DynamicImage, ImageBuffer, Rgba};
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        .or_else(|| twitter::extract_status_id(&opts.status))
    {
        let bytes = browser::twitter::shoot(
            &mut client,
            status_id,
            opts.width,
            opts.height,
            Some(Duration::from_millis(500)),
        )
        .await?;

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

        let img = image::load_from_memory(&bytes)?;
        img.save(full_path)?;

        let as_rgba = img.into_rgba();

        if let Some((x, y, w, h)) = clip(&as_rgba) {
            let clipping = DynamicImage::ImageRgba8(as_rgba).crop(x, y, w, h);
            clipping.save(crop_path)?;
        } else {
            eprintln!("Unable to crop tweet");
        }

        Ok(())
    } else {
        Err(twitter::Error::TweetIDParseError(opts.status).into())
    }
}

fn clip(buffer: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Option<(u32, u32, u32, u32)> {
    let w = buffer.width();
    let h = buffer.height();
    let mut left_edge = None;
    let mut right_edge = None;
    let mut gray = None;
    let white = Rgba([255u8, 255, 255, 255]);

    let mut i = 0;

    // Start at the upper-left corner and find the first intersecting line as you move right.
    while i < w {
        if *buffer.get_pixel(i, 0) != white {
            left_edge = Some(i + 2);
            gray = Some(buffer.get_pixel(i, 0));
            i += 2;
            break;
        }
        i += 1;
    }

    // Continue moving right to the second intersecting line.
    while i < w {
        if *buffer.get_pixel(i, 0) != white {
            right_edge = Some(i - 1);
            break;
        }
        i += 1;
    }

    left_edge
        .zip(right_edge)
        .zip(gray)
        .and_then(|((left, right), gray)| {
            i = 0;

            let mut upper_edge = None;
            let mut lower_edge = None;

            // Start at the top at the newly-discovered left edge, move down until the first line.
            while i < h {
                if *buffer.get_pixel(left, i) != white {
                    upper_edge = Some(i + 2);
                    i += 2;
                    break;
                }
                i += 1;
            }

            // And the next line, which represents the bottom of the tweet, including the actions.
            while i < h {
                if *buffer.get_pixel(left, i) != white {
                    lower_edge = Some(i - 1);
                    break;
                }
                i += 1;
            }

            upper_edge.zip(lower_edge).and_then(|(upper, lower)| {
                i = lower;

                let middle = left + (right - left) / 2;
                let mut base = None;

                // Finally move up until you hit another gray line.
                while i > 0 {
                    if buffer.get_pixel(middle, i) == gray {
                        base = Some(i - 2);
                        break;
                    }

                    i -= 1;
                }

                base.map(|b| (left, upper, right - left, b - upper))
            })
        })
}

#[derive(Clap)]
#[clap(version = crate_version!(), author = crate_authors!())]
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
