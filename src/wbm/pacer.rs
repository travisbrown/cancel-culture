use std::sync::Arc;
use std::time::Duration;

/// Create an opt-in `wayback_rs::Pacer` for cancel-culture.
///
/// This provides separate pacing hooks for the CDX API surface and for content
/// downloads, while remaining a small, self-contained change.
pub fn default_wayback_pacer() -> Arc<wayback_rs::Pacer> {
    // CDX: documented ~60 req/min. Start slightly conservative with a small burst.
    let cdx = Arc::new(
        leaky_bucket::RateLimiter::builder()
        .max(5)
        .initial(5)
        .interval(Duration::from_secs(1))
        .build(),
    );

    // Content: start conservative; tune independently from CDX.
    let content = Arc::new(
        leaky_bucket::RateLimiter::builder()
        .max(5)
        .initial(5)
        .interval(Duration::from_millis(1500))
        .build(),
    );

    Arc::new(wayback_rs::Pacer::new(
        move || {
            let cdx = Arc::clone(&cdx);
            async move {
                cdx.acquire_one().await;
            }
        },
        move || {
            let content = Arc::clone(&content);
            async move {
                content.acquire_one().await;
            }
        },
    ))
}


