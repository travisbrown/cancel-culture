use cancel_culture::browser;
use fantoccini::Locator;
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::sleep;

/// Super-low-tech way to download your block list without a Twitter API account.
#[tokio::main]
async fn main() -> Result<(), fantoccini::error::CmdError> {
    let client = browser::make_client_or_panic("firefox", false, None, None).await;

    client.goto(BLOCK_LIST_URL).await?;

    // This shouldn't really be necessary, but is for some reason?
    sleep(Duration::from_millis(500)).await;

    client
        .wait_for(move |c| {
            let mut cc = c.clone();
            async move {
                let url = cc.current_url().await?;
                Ok(url.as_str() == BLOCK_LIST_URL)
            }
        })
        .await?;

    sleep(Duration::from_millis(2000)).await;

    let mut account_list = client
        .wait()
        .forever()
        .for_element(ACCOUNT_LIST_LOC)
        .await?;
    let mut seen = HashSet::new();
    let mut attempts = 0;

    while attempts < MAX_ATTEMPTS {
        sleep(Duration::from_millis(500)).await;

        let account_items = account_list.find_all(ACCOUNT_ITEM_LOC).await?;
        let mut added = 0;

        for item in account_items {
            let link = item.find(ACCOUNT_LINK_LOC).await?;
            if let Some(name) = link
                .attr("href")
                .await?
                .as_ref()
                .and_then(|v| v.split('/').last())
            {
                if !seen.contains(name) {
                    println!("{}", name);
                    added += 1;
                    seen.insert(String::from(name));
                }
            }
        }

        if added == 0 {
            attempts += 1;
        } else {
            attempts = 0;
        }

        client.active_element().await?.send_keys(" ").await?;
    }

    Ok(())
}

const ACCOUNT_LIST_LOC: Locator = Locator::XPath("//div[@aria-label='Timeline: Blocked accounts']");
const ACCOUNT_ITEM_LOC: Locator = Locator::XPath("div/div/div/div[@data-testid='UserCell']");
const ACCOUNT_LINK_LOC: Locator = Locator::XPath("div/div/div/a[@role='link']");

const BLOCK_LIST_URL: &str = "https://twitter.com/settings/blocked/all";

// The number of times we press the spacebar without adding new accounts before stopping.
const MAX_ATTEMPTS: usize = 10;
