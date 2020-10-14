use core::hash::Hash;
use fantoccini::Client;
use futures::{Future, Stream, StreamExt};
use std::collections::HashSet;
use std::pin::Pin;

struct State<'a, F, Fut, Item, E>
where
    F: FnMut(&mut Client) -> Fut,
    Fut: Future<Output = Result<Vec<Item>, E>>,
{
    client: &'a mut Client,
    extract: F,
    remaining_attempts: usize,
    seen: HashSet<Item>,
}

pub fn scroll<'a, F: 'a, Fut: 'a, Item: Clone + Eq + Hash + 'static, E: 'static>(
    client: &'a mut Client,
    extract: F,
    max_attempts: usize,
) -> impl Stream<Item = Result<Item, E>> + 'a
where
    F: FnMut(&mut Client) -> Fut,
    Fut: Future<Output = Result<Vec<Item>, E>>,
{
    futures::stream::unfold(
        State {
            client,
            extract,
            remaining_attempts: max_attempts,
            seen: HashSet::new(),
        },
        move |s| async move {
            let State {
                client,
                mut extract,
                remaining_attempts,
                mut seen,
            } = s;

            if remaining_attempts > 0 {
                let mut res = extract(client).await;

                match res {
                    Ok(ref mut values) => {
                        values.retain(|item| !seen.contains(item));

                        if values.is_empty() {
                            Some((
                                res,
                                State {
                                    client,
                                    extract,
                                    remaining_attempts: remaining_attempts - 1,
                                    seen,
                                },
                            ))
                        } else {
                            for value in values {
                                seen.insert(value.clone());
                            }
                            Some((
                                res,
                                State {
                                    client,
                                    extract,
                                    remaining_attempts: max_attempts,
                                    seen,
                                },
                            ))
                        }
                    }
                    Err(_) => Some((
                        res,
                        State {
                            client,
                            extract,
                            remaining_attempts: 0,
                            seen,
                        },
                    )),
                }
            } else {
                None
            }
        },
    )
    .flat_map(|res| {
        let as_stream: Pin<Box<dyn Stream<Item = Result<Item, E>>>> = match res {
            Ok(values) => Box::pin(futures::stream::iter(values.into_iter().map(Ok))),
            Err(e) => Box::pin(futures::stream::once(async { Err(e) })),
        };

        as_stream
    })
}
