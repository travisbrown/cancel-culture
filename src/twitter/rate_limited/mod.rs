mod future;
mod method_limit;
mod stream;

pub use stream::RateLimitedStream;

use super::{Method, ResponseFuture};
use method_limit::{MethodLimit, MethodLimits};

use egg_mode::cursor::{Cursor, CursorIter};
use egg_mode::error::Result;
use egg_mode::service::rate_limit_status;
use egg_mode::Token;
use serde::de::DeserializeOwned;
use std::iter::Peekable;

pub(crate) struct RateLimitedClient {
    limits: MethodLimits,
}

impl RateLimitedClient {
    pub(crate) async fn new(token: Token) -> Result<RateLimitedClient> {
        let status = rate_limit_status(&token).await?.response;
        let limits = MethodLimits::from(status);

        Ok(RateLimitedClient { limits })
    }

    pub(crate) fn cursor_stream<T: Cursor + DeserializeOwned + 'static>(
        &self,
        method: &Method,
        underlying: CursorIter<T>,
    ) -> RateLimitedStream<'static, CursorIter<T>>
    where
        T::Item: Unpin + Send,
    {
        RateLimitedStream::new(underlying, self.limits.get(method))
    }

    pub(crate) fn futures_stream<'a, T: 'static, I: Iterator<Item = ResponseFuture<'a, Vec<T>>>>(
        &self,
        method: &Method,
        iterator: I,
    ) -> RateLimitedStream<'a, Peekable<I>> {
        RateLimitedStream::new(iterator.peekable(), self.limits.get(method))
    }
}
