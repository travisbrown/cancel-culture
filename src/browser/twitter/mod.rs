use fantoccini::error::CmdError;
use fantoccini::{Client, Locator};
use image::{DynamicImage, GenericImageView, Rgba};
use std::time::Duration;

const HEADING_LOC: Locator = Locator::XPath("//main//h1[@role='heading']");

pub async fn status_exists(client: &mut Client, id: u64) -> Result<bool, CmdError> {
    let url = format!("https://twitter.com/tweet/status/{}", id);

    client.goto(&url).await?;
    let mut heading = client.wait_for_find(HEADING_LOC).await?;

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

    let mut username_input = client
        .wait_for_find(Locator::Css("input[name='session[username_or_email]']"))
        .await?;
    username_input.send_keys(username).await?;

    let mut password_input = client
        .wait_for_find(Locator::Css("input[name='session[password]']"))
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
    client.wait_for_find(locator).await?;

    if let Some(duration) = wait_for_load {
        tokio::time::delay_for(duration).await;
    }

    client.screenshot().await
}

quick_error! {
    #[derive(Debug)]
    pub enum ScreenshotError {
        DownloadError(err: fantoccini::error::CmdError) { from() }
        ImageDecodingError(err: image::error::ImageError) { from() }
    }
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
        if buffer.get_pixel(i, 0) != RGBA_WHITE {
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
                if buffer.get_pixel(left, i) != RGBA_WHITE {
                    upper_edge = Some(i + 2);
                    i += 2;
                    break;
                }
                i += 1;
            }

            // And the next line, which represents the bottom of the tweet, including the actions.
            while i < h {
                if buffer.get_pixel(left, i) != RGBA_WHITE {
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
