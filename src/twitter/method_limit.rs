use super::Method;

use chrono::{DateTime, TimeZone, Utc};
use egg_mode::service::RateLimitStatus;
use egg_mode::{RateLimit, Response};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
/// Rate limit information about a single API method.
pub struct MethodLimit {
    remaining: AtomicI32,
    reset: AtomicI32,
}

impl MethodLimit {
    // This is pretty ad-hoc. It needs to be less than 15 minutes but greater than 0.
    const SAME_RESET_TOLERANCE_SECONDS: i32 = 30;

    // Not very performance-sensitive so let's just be safe.
    const DEFAULT_ORDERING: Ordering = Ordering::SeqCst;

    // Being careful about clock differences.
    const WAIT_BUFFER_SECONDS: i64 = 10;

    /// The number of remaining requests before the next reset.
    pub fn remaining(&self) -> i32 {
        self.remaining.load(Self::DEFAULT_ORDERING)
    }

    fn reset_timestamp(&self) -> i32 {
        self.reset.load(Self::DEFAULT_ORDERING)
    }

    /// The time of the next reset.
    pub fn reset_time(&self) -> DateTime<Utc> {
        Utc.timestamp(self.reset_timestamp().into(), 0)
    }

    /// Use a request.
    pub fn decrement(&self) {
        self.remaining.fetch_sub(1, Self::DEFAULT_ORDERING);
    }

    /// Update with rate limit information from the Twitter API.
    pub fn update(&self, remaining: i32, reset: i32) {
        let old_reset = self.reset_timestamp();

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

    /// The amount of time until the next reset if there are no remaining requests.
    pub fn wait_duration(&self) -> Option<Duration> {
        if self.remaining() > 0 {
            None
        } else {
            let difference: i64 =
                self.reset_timestamp() as i64 - Utc::now().timestamp() + Self::WAIT_BUFFER_SECONDS;

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

/// Rate limit information for all methods for a single token.
pub struct MethodLimitStore(HashMap<Method, Arc<MethodLimit>>);

impl MethodLimitStore {
    fn wrap<M: Into<Method>>(pair: (M, Response<()>)) -> (Method, Arc<MethodLimit>) {
        (
            pair.0.into(),
            Arc::new(MethodLimit::from(&pair.1.rate_limit_status)),
        )
    }

    /// Look up rate limit information for a method.
    pub fn get(&self, method: &Method) -> Arc<MethodLimit> {
        self.0
            .get(method)
            .expect("Method not yet tracked by limit-aware client")
            .clone()
    }
}

impl From<RateLimitStatus> for MethodLimitStore {
    fn from(status: RateLimitStatus) -> MethodLimitStore {
        let mut limits = HashMap::new();

        limits.extend(status.direct.into_iter().map(Self::wrap));
        limits.extend(status.list.into_iter().map(Self::wrap));
        limits.extend(status.place.into_iter().map(Self::wrap));
        limits.extend(status.search.into_iter().map(Self::wrap));
        limits.extend(status.service.into_iter().map(Self::wrap));
        limits.extend(status.tweet.into_iter().map(Self::wrap));
        limits.extend(status.user.into_iter().map(Self::wrap));

        MethodLimitStore(limits)
    }
}
