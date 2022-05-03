pub mod parser;
pub mod search;
mod tweet_lister;
pub use tweet_lister::TweetLister;

use fantoccini::error::CmdError;
use fantoccini::{Client, Locator};
use image::{DynamicImage, GenericImageView, Rgba};
use std::time::Duration;

const HEADING_LOC: Locator = Locator::XPath("//main//h1[@role='heading']");

pub async fn status_exists(client: &mut Client, id: u64) -> Result<bool, CmdError> {
    let url = format!("https://twitter.com/tweet/status/{}", id);

    client.goto(&url).await?;
    let heading = client.wait().forever().for_element(HEADING_LOC).await?;

    Ok(heading
        .attr("data-testid")
        .await?
        .map_or(true, |v| v != "error-detail"))
}

pub async fn is_logged_in(client: &mut Client) -> Result<bool, CmdError> {
    client.goto("https://twitter.com/login").await?;
    let current = client.current_url().await?;
    Ok(current.as_str() == "https://twitter.com/home")
}

pub async fn log_in(client: &mut Client, username: &str, password: &str) -> Result<bool, CmdError> {
    client.goto("https://twitter.com/login").await?;

    let username_input = client
        .wait()
        .forever()
        .for_element(Locator::Css("input[name='session[username_or_email]']"))
        .await?;
    username_input.send_keys(username).await?;

    let password_input = client
        .wait()
        .forever()
        .for_element(Locator::Css("input[name='session[password]']"))
        .await?;
    password_input
        .send_keys(&(String::from(password) + "\n"))
        .await?;

    is_logged_in(client).await
}

pub async fn shoot_tweet_bytes(
    client: &mut Client,
    status_id: u64,
    width: u32,
    height: u32,
    wait_for_load: Option<Duration>,
) -> Result<Vec<u8>, fantoccini::error::CmdError> {
    client.set_window_size(width, height).await?;

    let url = format!("https://twitter.com/tweet/status/{}", status_id);
    client.goto(&url).await?;

    let locator = fantoccini::Locator::XPath("//main//h1[@role='heading']");
    client.wait().forever().for_element(locator).await?;

    if let Some(duration) = wait_for_load {
        tokio::time::sleep(duration).await;
    }

    // There may be a cookies layer. If so we hide it.
    client
        .execute(
            "document.getElementById('layers').children[0].style.display = 'none';",
            vec![],
        )
        .await?;

    client.screenshot().await
}

#[derive(thiserror::Error, Debug)]
pub enum ScreenshotError {
    #[error("Download error")]
    Download(#[from] fantoccini::error::CmdError),
    #[error("Image decoding error")]
    ImageDecoding(#[from] image::error::ImageError),
}

pub async fn shoot_tweet(
    client: &mut Client,
    status_id: u64,
    width: u32,
    height: u32,
    wait_for_load: Option<Duration>,
) -> Result<DynamicImage, ScreenshotError> {
    let bytes = shoot_tweet_bytes(client, status_id, width, height, wait_for_load).await?;

    Ok(image::load_from_memory(&bytes)?)
}

const RGBA_WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

// TODO: Figure out why this is necessary for finding the right edge in some cases.
fn is_very_light_gray(pixel: &Rgba<u8>) -> bool {
    let threshhold = 253;
    pixel.0[0] >= threshhold && pixel.0[1] >= threshhold && pixel.0[2] >= threshhold
}

pub fn crop_tweet<I: GenericImageView<Pixel = Rgba<u8>>>(
    buffer: &I,
) -> Option<(u32, u32, u32, u32)> {
    let w = buffer.width();
    let h = buffer.height();
    let mut left_edge = None;
    let mut right_edge = None;
    let mut gray = None;

    let mut i = 0;

    // Start at the upper-left corner and find the first intersecting line as you move right.
    while i < w {
        if buffer.get_pixel(i, 0) != RGBA_WHITE {
            left_edge = Some(i + 2);
            gray = Some(buffer.get_pixel(i, 0));
            i += 2;
            break;
        }
        i += 1;
    }

    // Continue moving right to the second intersecting line.
    while i < w {
        let pixel = buffer.get_pixel(i, 0);

        if !is_very_light_gray(&pixel) {
            right_edge = Some(i - 1);
            break;
        }
        i += 1;
    }

    left_edge
        .zip(right_edge)
        .zip(gray)
        .and_then(|((left, right), gray)| {
            let mut upper_edge = None;
            let mut lower_edge = None;
            let mut i = 0;

            // We no longer have a top border, so we find the top of the profile image and count up from there.
            // This is a terrible hack and needs to be improved.

            // Find the top of the text above the profile image.
            while i < h {
                if (left..=right)
                    .map(|j| buffer.get_pixel(j, i))
                    .any(|p| p != RGBA_WHITE)
                {
                    break;
                }
                i += 1;
            }

            // Find the base of the text.
            while i < h {
                if !(left..=right)
                    .map(|j| buffer.get_pixel(j, i))
                    .any(|p| p != RGBA_WHITE)
                {
                    break;
                }
                i += 1;
            }
            let text_base = i;

            // Find the top of the profile image.
            while i < h {
                if (left..=right)
                    .map(|j| buffer.get_pixel(j, i))
                    .any(|p| p != RGBA_WHITE)
                {
                    break;
                }
                i += 1;
            }

            upper_edge = Some(text_base + (i - text_base) / 2);

            let mut i = 0;

            // The first line represents the bottom of the tweet, including the actions.
            while i < h {
                if buffer.get_pixel(left, i) != RGBA_WHITE {
                    lower_edge = Some(i - 1);
                    break;
                }
                i += 1;
            }

            upper_edge.zip(lower_edge).and_then(|(upper, lower)| {
                // We move up two pixels because of a new double line.
                // This should be fairly robust, since the target will always be higher anyway.
                i = lower - 2;

                let middle = left + (right - left) / 2;
                let mut base = None;

                // Finally move up until you hit another gray line.
                while i > 0 {
                    if buffer.get_pixel(middle, i) != RGBA_WHITE {
                        base = Some(i - 2);
                        break;
                    }

                    i -= 1;
                }

                base.map(|b| (left, upper, right - left, b - upper))
            })
        })
}

#[cfg(test)]
mod tests {
    use image::io::Reader;
    use image::RgbaImage;
    use std::path::Path;

    fn load_image<P: AsRef<Path>>(path: P) -> RgbaImage {
        Reader::open(path).unwrap().decode().unwrap().into_rgba8()
    }

    #[test]
    fn crop_tweet() {
        let examples = vec![
            (
                "examples/images/703033780689199104-full.png",
                Some((253, 99, 1195, 494)),
            ),
            (
                "examples/images/1503631923154984960-full.png",
                Some((253, 99, 1195, 1184)),
            ),
        ];

        for (path, expected) in examples {
            assert_eq!(super::crop_tweet(&load_image(path)), expected);
        }
    }
}
