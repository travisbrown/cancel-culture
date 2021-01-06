use super::Result;
use fantoccini::{Client as FClient, Locator};
use futures::{future::LocalBoxFuture, FutureExt};
use std::time::Duration;
use tokio::time::sleep;

pub struct Client {
    underlying: FClient,
}

impl Client {
    const LOGIN_URL: &'static str = "https://archive.org/account/login";
    const SAVE_URL: &'static str = "https://web.archive.org/save";
    const LOGIN_FORM_LOC: Locator<'static> = Locator::Css("form[name='login-form']");
    const SAVE_FORM_LOC: Locator<'static> = Locator::Css("#web-save-form");
    const SAVE_DONE_LOC: Locator<'static> = Locator::XPath(
        "//div[@id='spn-result']/span/a[contains(@href, '/web/')] | //div[@id='spn-result']/p[@class='text-danger']"
    );
    const SAVE_WAIT_MILLIS: u64 = 1000;

    pub fn new(client: FClient) -> Client {
        Client { underlying: client }
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Result<()> {
        self.underlying.goto(Self::LOGIN_URL).await?;
        let mut form = self.underlying.form(Self::LOGIN_FORM_LOC).await?;
        form.set_by_name("username", username)
            .await?
            .set_by_name("password", password)
            .await?
            .submit()
            .await?;

        Ok(())
    }

    pub fn save<'a>(&'a mut self, url: &'a str) -> LocalBoxFuture<'a, Result<String>> {
        async move {
            sleep(Duration::from_millis(Self::SAVE_WAIT_MILLIS)).await;
            self.underlying.goto(Self::SAVE_URL).await?;

            self.underlying.wait_for_find(Self::SAVE_FORM_LOC).await?;
            let mut form = self.underlying.form(Self::SAVE_FORM_LOC).await?;
            form.set_by_name("url", url)
                .await?
                .set_by_name("capture_screenshot", "on")
                .await?
                .set_by_name("wm-save-mywebarchive", "on")
                .await?
                .set_by_name("email_result", "on")
                .await?
                .submit()
                .await?;

            let mut result = self.underlying.wait_for_find(Self::SAVE_DONE_LOC).await?;
            let result_href = result.attr("href").await?;

            match result_href {
                Some(result_url) => {
                    // This is usually because Twitter redirects the Wayback Machine to e.g. a
                    // hashflags URL for some reason. It should be okay to retry after 30 minutes.
                    if !result_url.contains(url) {
                        log::warn!("Save failed for {}", url);
                    }

                    Ok(result_url)
                }
                None => self.save(url).await,
            }
        }
        .boxed_local()
    }
}
