mod method_limit;

use method_limit::{MethodLimit, MethodLimits};

use chrono::{TimeZone, Utc};
use egg_mode::cursor::{Cursor, CursorIter};
use egg_mode::service::rate_limit_status;
use egg_mode::{Response, Token};
use futures::{Future, Stream};
use log::warn;
use serde::de::DeserializeOwned;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::{delay_for, Delay};

pub(crate) struct RateLimitedClient {
    is_user_token: bool,
    limits: MethodLimits,
}

impl RateLimitedClient {
    pub(crate) async fn new(
        token: Token,
        is_user_token: bool,
    ) -> egg_mode::error::Result<RateLimitedClient> {
        let status = rate_limit_status(&token).await?.response;
        let limits = MethodLimits::from(status);

        Ok(RateLimitedClient {
            is_user_token,
            limits,
        })
    }

    pub(crate) fn make_stream<T: Cursor + DeserializeOwned>(
        &self,
        method: &super::Method,
        underlying: CursorIter<T>,
    ) -> RateLimitedStream<T>
    where
        T::Item: Unpin + Send,
    {
        RateLimitedStream {
            underlying,
            limit: self.limits.get(method),
            state: None,
        }
    }
}

type FutureResponse<T> = Pin<Box<dyn Future<Output = egg_mode::error::Result<Response<T>>> + Send>>;

enum StreamState<T: Cursor + DeserializeOwned>
where
    T::Item: Unpin + Send,
{
    Waiting(Delay),
    Loading(FutureResponse<T>),
    Iterating(Box<dyn Iterator<Item = T::Item>>),
}

pub struct RateLimitedStream<T: Cursor + DeserializeOwned>
where
    T::Item: Unpin + Send,
{
    underlying: CursorIter<T>,
    limit: Arc<MethodLimit>,
    state: Option<StreamState<T>>,
}

impl<T: Cursor + DeserializeOwned + 'static> Stream for RateLimitedStream<T>
where
    T::Item: Unpin + Send,
{
    type Item = egg_mode::error::Result<T::Item>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Some(mut state) = self.state.take() {
            match state {
                StreamState::Waiting(ref mut fut) => match Pin::new(fut).poll(cx) {
                    Poll::Pending => {
                        self.state = Some(state);
                        Poll::Pending
                    }
                    Poll::Ready(()) => self.poll_next(cx),
                },
                StreamState::Loading(ref mut fut) => match Pin::new(fut).poll(cx) {
                    Poll::Pending => {
                        self.state = Some(state);
                        Poll::Pending
                    }
                    Poll::Ready(Ok(res)) => {
                        self.underlying.previous_cursor = res.previous_cursor_id();
                        self.underlying.next_cursor = res.next_cursor_id();

                        self.limit
                            .update(res.rate_limit_status.remaining, res.rate_limit_status.reset);

                        let mut items = res.response.into_inner().into_iter();
                        let first = items.next();

                        self.state = Some(StreamState::Iterating(Box::new(items)));
                        Poll::Ready(first.map(Ok))
                    }
                    Poll::Ready(Err(e)) => {
                        self.state = Some(state);
                        Poll::Ready(Some(Err(e)))
                    }
                },
                StreamState::Iterating(ref mut iter) => {
                    if let Some(item) = iter.next() {
                        self.state = Some(state);
                        Poll::Ready(Some(Ok(item)))
                    } else if self.underlying.next_cursor == 0 {
                        self.state = Some(state);
                        Poll::Ready(None)
                    } else {
                        self.poll_next(cx)
                    }
                }
            }
        } else {
            let state = if let Some(delay) = self.limit.delay() {
                warn!(
                    "Waiting for {:?} for rate limit reset at {:?}",
                    delay,
                    Utc.timestamp(self.limit.reset().into(), 0)
                );
                StreamState::Waiting(delay_for(delay))
            } else {
                self.limit.decrement();
                StreamState::Loading(Box::pin(self.underlying.call()))
            };

            self.state = Some(state);
            self.poll_next(cx)
        }
    }
}
