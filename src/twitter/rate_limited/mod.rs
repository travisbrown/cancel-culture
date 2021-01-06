mod stream;

use super::{Method, MethodLimitStore};
pub use stream::{Pageable, TimelineScrollback};

use egg_mode::{error::Result, service::rate_limit_status, Token};
use futures::{stream::LocalBoxStream, StreamExt, TryStreamExt};
use log::warn;
use tokio::time::sleep;

pub(crate) struct RateLimitedClient {
    limits: MethodLimitStore,
}

impl RateLimitedClient {
    pub(crate) async fn new(token: Token) -> Result<RateLimitedClient> {
        let status = rate_limit_status(&token).await?.response;
        let limits = MethodLimitStore::from(status);

        Ok(RateLimitedClient { limits })
    }

    pub(crate) fn make_stream<'a, L: Pageable<'a> + 'a>(
        &self,
        loader: L,
        method: &Method,
    ) -> LocalBoxStream<'a, Result<L::Item>> {
        let limit = self.limits.get(method);

        futures::stream::try_unfold((loader, false), move |(mut this, is_done)| {
            let limit = limit.clone();
            async move {
                if is_done {
                    let res: Result<Option<_>> = Ok(None);
                    res
                } else {
                    if let Some(delay) = limit.wait_duration() {
                        warn!(
                            "Waiting for {:?} for rate limit reset at {:?}",
                            delay,
                            limit.reset_time()
                        );
                        sleep(delay).await;
                    }

                    limit.decrement();
                    let mut response = this.load().await?;
                    let is_done = this.update(&mut response);

                    limit.update(
                        response.rate_limit_status.remaining,
                        response.rate_limit_status.reset,
                    );

                    Ok(Some((L::extract(response.response), (this, is_done))))
                }
            }
        })
        .map_ok(|items| futures::stream::iter(items).map(Ok))
        .try_flatten()
        .boxed_local()
    }
}
