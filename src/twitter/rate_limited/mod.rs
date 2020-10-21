mod method_limit;
mod stream;

pub use stream::{Pageable, TimelineScrollback};

use super::Method;
use method_limit::MethodLimits;

use chrono::{TimeZone, Utc};
use egg_mode::error::Result;
use egg_mode::service::rate_limit_status;
use egg_mode::Token;
use futures::{stream::LocalBoxStream, StreamExt, TryStreamExt};
use log::warn;
use tokio::time::delay_for;

pub(crate) struct RateLimitedClient {
    limits: MethodLimits,
}

impl RateLimitedClient {
    pub(crate) async fn new(token: Token) -> Result<RateLimitedClient> {
        let status = rate_limit_status(&token).await?.response;
        let limits = MethodLimits::from(status);

        Ok(RateLimitedClient { limits })
    }

    pub(crate) fn to_stream<'a, L: Pageable<'a> + 'a>(
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
                    if let Some(delay) = limit.delay() {
                        warn!(
                            "Waiting for {:?} for rate limit reset at {:?}",
                            delay,
                            Utc.timestamp(limit.reset().into(), 0)
                        );
                        delay_for(delay).await;
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
