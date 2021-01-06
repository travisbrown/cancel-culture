use egg_mode::{
    error::Result,
    tweet::{Timeline, Tweet},
};
use futures::{
    stream::LocalBoxStream,
    stream::{iter, try_unfold},
    StreamExt, TryStreamExt,
};
use std::time::Duration;
use tokio::time::sleep;

pub fn make_stream(timeline: Timeline, wait: Duration) -> LocalBoxStream<'static, Result<Tweet>> {
    try_unfold(
        (timeline, None, false),
        move |(timeline, mut max_id, mut started)| async move {
            let (mut timeline, response) = if !started {
                started = true;
                timeline.start().await?
            } else {
                sleep(wait).await;
                timeline.newer(None).await?
            };

            if !response.is_empty() {
                max_id = timeline.max_id;
            } else {
                timeline.max_id = max_id;
            }

            let res: Result<Option<_>> = Ok(Some((response.response, (timeline, max_id, started))));
            res
        },
    )
    .map_ok(|vs| iter(vs).map(Ok))
    .try_flatten()
    .boxed_local()
}
