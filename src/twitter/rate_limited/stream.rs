use super::super::ResponseFuture;
use super::MethodLimit;

use chrono::{TimeZone, Utc};
use egg_mode::cursor::{Cursor, CursorIter};
use egg_mode::error::Result;
use egg_mode::Response;
use futures::{Future, Stream};
use log::warn;
use serde::de::DeserializeOwned;
use std::iter::Peekable;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::{delay_for, Delay};

pub trait Loader<'a> {
    type Item: 'static;
    type Page;

    fn load(&mut self) -> ResponseFuture<'a, Self::Page>;
    fn update(&mut self, response: &Response<Self::Page>);
    fn extract(page: Self::Page) -> Vec<Self::Item>;
    fn is_done(&mut self) -> bool;
}

impl<'a, T: 'static, I: Iterator<Item = ResponseFuture<'a, Vec<T>>>> Loader<'a> for Peekable<I> {
    type Item = T;
    type Page = Vec<T>;

    fn load(&mut self) -> ResponseFuture<'a, Self::Page> {
        self.next().unwrap()
    }
    fn update(&mut self, _response: &Response<Self::Page>) {}
    fn extract(page: Self::Page) -> Vec<Self::Item> {
        page
    }
    fn is_done(&mut self) -> bool {
        self.peek().is_none()
    }
}

impl<T: Cursor + DeserializeOwned + 'static> Loader<'static> for CursorIter<T> {
    type Item = T::Item;
    type Page = T;

    fn load(&mut self) -> ResponseFuture<'static, Self::Page> {
        Box::pin(self.call())
    }
    fn update(&mut self, response: &Response<Self::Page>) {
        self.previous_cursor = response.previous_cursor_id();
        self.next_cursor = response.next_cursor_id();
    }
    fn extract(page: Self::Page) -> Vec<Self::Item> {
        page.into_inner()
    }
    fn is_done(&mut self) -> bool {
        self.next_cursor == 0
    }
}

enum StreamState<'a, L: Loader<'a>> {
    Waiting(Delay),
    Loading(ResponseFuture<'a, L::Page>),
    Iterating(Box<dyn Iterator<Item = L::Item>>),
}

pub struct RateLimitedStream<'a, L: Loader<'a>>
where
    L::Item: 'static,
{
    underlying: L,
    limit: Arc<MethodLimit>,
    state: Option<StreamState<'a, L>>,
}

impl<'a, L: Loader<'a>> RateLimitedStream<'a, L> {
    pub(crate) fn new(underlying: L, limit: Arc<MethodLimit>) -> RateLimitedStream<'a, L> {
        RateLimitedStream {
            underlying,
            limit,
            state: None,
        }
    }
}

impl<'a, L: Loader<'a> + Unpin> Stream for RateLimitedStream<'a, L> {
    type Item = Result<L::Item>;

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
                        self.underlying.update(&res);

                        self.limit
                            .update(res.rate_limit_status.remaining, res.rate_limit_status.reset);

                        let mut items = L::extract(res.response).into_iter();
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
                    } else if self.underlying.is_done() {
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
                StreamState::Loading(self.underlying.load())
            };

            self.state = Some(state);
            self.poll_next(cx)
        }
    }
}
