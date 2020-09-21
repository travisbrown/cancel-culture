use fantoccini::error::CmdError;
use fantoccini::{Client, Locator};

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
