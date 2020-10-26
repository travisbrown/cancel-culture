use egg_mode::{
    cursor::{Cursor, CursorIter},
    error::{Error, Result},
    tweet::{Timeline, Tweet},
    Response,
};
use futures::{
    future::{err, LocalBoxFuture},
    FutureExt, TryFutureExt,
};
use serde::de::DeserializeOwned;
use std::iter::Peekable;

type ResponseFuture<'a, T> = LocalBoxFuture<'a, Result<Response<T>>>;

pub trait Pageable<'a> {
    type Item: 'static;
    type Page;

    fn load(&mut self) -> ResponseFuture<'a, Self::Page>;
    fn update(&mut self, response: &mut Response<Self::Page>) -> bool;
    fn extract(page: Self::Page) -> Vec<Self::Item>;
}

impl<'a, T: 'static, I: Iterator<Item = ResponseFuture<'a, Vec<T>>>> Pageable<'a> for Peekable<I> {
    type Item = T;
    type Page = Vec<T>;

    fn load(&mut self) -> ResponseFuture<'a, Self::Page> {
        self.next().unwrap()
    }
    fn update(&mut self, _response: &mut Response<Self::Page>) -> bool {
        self.peek().is_none()
    }
    fn extract(page: Self::Page) -> Vec<Self::Item> {
        page
    }
}

impl<T: Cursor + DeserializeOwned + 'static> Pageable<'static> for CursorIter<T> {
    type Item = T::Item;
    type Page = T;

    fn load(&mut self) -> ResponseFuture<'static, Self::Page> {
        self.call().boxed_local()
    }
    fn update(&mut self, response: &mut Response<Self::Page>) -> bool {
        self.previous_cursor = response.previous_cursor_id();
        self.next_cursor = response.next_cursor_id();
        self.next_cursor == 0
    }
    fn extract(page: Self::Page) -> Vec<Self::Item> {
        page.into_inner()
    }
}

pub struct TimelineScrollback {
    timeline: Option<Timeline>,
    is_started: bool,
}

impl TimelineScrollback {
    pub fn new(timeline: Timeline) -> TimelineScrollback {
        TimelineScrollback {
            timeline: Some(timeline),
            is_started: false,
        }
    }

    fn lift_response(
        pair: (Timeline, Response<Vec<Tweet>>),
    ) -> Response<(Option<Timeline>, Vec<Tweet>)> {
        let (timeline, response) = pair;
        Response::map(response, |tweets| (Some(timeline), tweets))
    }
}

impl Pageable<'static> for TimelineScrollback {
    type Item = Tweet;
    type Page = (Option<Timeline>, Vec<Tweet>);

    fn load(&mut self) -> ResponseFuture<'static, Self::Page> {
        if let Some(timeline) = self.timeline.take() {
            if self.is_started {
                timeline
                    .older(None)
                    .map_ok(TimelineScrollback::lift_response)
                    .boxed_local()
            } else {
                self.is_started = true;
                timeline
                    .start()
                    .map_ok(TimelineScrollback::lift_response)
                    .boxed_local()
            }
        } else {
            // This shouldn't happen.
            err(Error::InvalidResponse("Problem retrieving timeline", None)).boxed_local()
        }
    }
    fn update(&mut self, response: &mut Response<Self::Page>) -> bool {
        self.timeline = response.response.0.take();
        response.response.1.is_empty()
    }
    fn extract(page: Self::Page) -> Vec<Self::Item> {
        page.1
    }
}
