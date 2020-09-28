use egg_mode::error::Result;
use egg_mode::tweet::{Timeline, Tweet};
use futures::{FutureExt, Stream, StreamExt, TryStreamExt};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::delay_for;

type TimelineResult = Result<(Timeline, egg_mode::Response<Vec<Tweet>>)>;

pub struct TimelineStream {
    underlying: Pin<Box<dyn Future<Output = TimelineResult>>>,
    wait: Duration,
    max_id: Option<u64>,
}

impl TimelineStream {
    pub fn new(timeline: Timeline, wait: Duration) -> TimelineStream {
        TimelineStream {
            underlying: Box::pin(timeline.start()),
            wait,
            max_id: None,
        }
    }

    pub fn make(timeline: Timeline, wait: Duration) -> Pin<Box<dyn Stream<Item = Result<Tweet>>>> {
        let base = TimelineStream::new(timeline, wait);

        base.map_ok(|vs| futures::stream::iter(vs).map(Ok))
            .try_flatten()
            .boxed_local()
    }
}

impl Stream for TimelineStream {
    type Item = Result<Vec<Tweet>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Future::poll(Pin::new(&mut self.underlying), cx)
            .map_ok(|(mut timeline, response)| {
                if !response.is_empty() {
                    self.max_id = timeline.max_id;
                } else {
                    timeline.max_id = self.max_id;
                }
                self.underlying = Box::pin(delay_for(self.wait).then(|_| timeline.newer(None)));
                response.response
            })
            .map(Some)
    }
}
