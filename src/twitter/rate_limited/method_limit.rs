use crate::twitter::Method;

use chrono::Utc;
use egg_mode::service::RateLimitStatus;
use egg_mode::{RateLimit, Response};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct MethodLimit {
    remaining: AtomicI32,
    reset: AtomicI32,
}

impl MethodLimit {
    // This is pretty ad-hoc. It needs to be less than 15 minutes but greater than 0.
    const SAME_RESET_TOLERANCE_SECONDS: i32 = 30;

    // Not very performance-sensitive so let's just be safe.
    const DEFAULT_ORDERING: Ordering = Ordering::SeqCst;

    // Being careful about clock differences.
    const DELAY_BUFFER_SECONDS: i64 = 10;

    pub(crate) fn remaining(&self) -> i32 {
        self.remaining.load(Self::DEFAULT_ORDERING)
    }

    pub(crate) fn reset(&self) -> i32 {
        self.reset.load(Self::DEFAULT_ORDERING)
    }

    pub(crate) fn decrement(&self) {
        self.remaining.fetch_sub(1, Self::DEFAULT_ORDERING);
    }

    pub(crate) fn update(&self, remaining: i32, reset: i32) {
        let old_reset = self.reset();

        if reset - old_reset > Self::SAME_RESET_TOLERANCE_SECONDS {
            // The update reset is more recent.
            if self
                .reset
                .compare_and_swap(old_reset, reset, Self::DEFAULT_ORDERING)
                == old_reset
            {
                // Only update the remaining count if the reset hasn't been updated in the meantime.
                self.remaining.store(remaining, Self::DEFAULT_ORDERING);
            }
        } else {
            // Still on the old reset, so we use the lowest remaining count to be safe.
            self.remaining.fetch_min(remaining, Self::DEFAULT_ORDERING);
        }
    }

    pub(crate) fn delay(&self) -> Option<Duration> {
        if self.remaining() > 0 {
            None
        } else {
            let difference: i64 =
                self.reset() as i64 - Utc::now().timestamp() + Self::DELAY_BUFFER_SECONDS;

            if difference <= 0 {
                None
            } else {
                Some(Duration::from_secs(difference as u64))
            }
        }
    }
}

impl From<&RateLimit> for MethodLimit {
    fn from(limit: &RateLimit) -> Self {
        MethodLimit {
            remaining: AtomicI32::new(limit.remaining),
            reset: AtomicI32::new(limit.reset),
        }
    }
}

pub(crate) struct MethodLimits(HashMap<Method, Arc<MethodLimit>>);

impl MethodLimits {
    fn wrap<M: Into<Method>>(pair: (M, Response<()>)) -> (Method, Arc<MethodLimit>) {
        (
            pair.0.into(),
            Arc::new(MethodLimit::from(&pair.1.rate_limit_status)),
        )
    }

    pub(crate) fn get(&self, method: &Method) -> Arc<MethodLimit> {
        self.0
            .get(method)
            .expect("Method not yet tracked by limit-aware client")
            .clone()
    }
}

impl From<RateLimitStatus> for MethodLimits {
    fn from(status: RateLimitStatus) -> MethodLimits {
        let mut limits = HashMap::new();

        limits.extend(status.direct.into_iter().map(MethodLimits::wrap));
        limits.extend(status.list.into_iter().map(MethodLimits::wrap));
        limits.extend(status.place.into_iter().map(MethodLimits::wrap));
        limits.extend(status.search.into_iter().map(MethodLimits::wrap));
        limits.extend(status.service.into_iter().map(MethodLimits::wrap));
        limits.extend(status.tweet.into_iter().map(MethodLimits::wrap));
        limits.extend(status.user.into_iter().map(MethodLimits::wrap));

        MethodLimits(limits)
    }
}
