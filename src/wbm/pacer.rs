use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum WaybackPacingProfile {
    /// Conservative pacing intended to minimize the risk of Wayback CDX throttling.
    Conservative,
    /// Default pacing (balanced).
    Default,
    /// Adaptive pacing with hysteresis (slow recovery, fast backoff).
    Adaptive,
}

/// Static pacing parameters (token bucket / leaky bucket).
#[derive(Clone, Copy, Debug)]
struct StaticPacingConfig {
    cdx_interval: Duration,
    cdx_burst: usize,
    content_interval: Duration,
    content_burst: usize,
}

/// Tuning parameters for adaptive pacing.
///
/// The goal is long-running stability: slow recovery, fast backoff, and
/// hysteresis (cooldown windows) to avoid flapping.
#[derive(Clone, Copy, Debug)]
struct AdaptiveConfig {
    /// Whether to enable a "slow start" phase (like TCP) after startup or after
    /// backpressure events.
    ///
    /// In slow start, we *reduce* the interval multiplicatively on each success
    /// (i.e., ramp up request rate quickly) until we cross a threshold, then we
    /// switch to additive recovery using `success_step`.
    slow_start: bool,
    /// Interval threshold at or below which we exit slow start and switch to
    /// additive recovery.
    slow_start_threshold: Duration,
    /// Divisor applied to the interval during slow start. A value of 2 means
    /// "double the speed" (halve the interval) on each success.
    slow_start_divisor: u32,

    /// Small additive recovery step applied on sustained success.
    success_step: Duration,
    /// Multiplicative backoff factor applied on backpressure (interval *= factor).
    backoff_factor: u32,

    /// CDX pacing bounds and initial interval.
    cdx_min_interval: Duration,
    cdx_initial_interval: Duration,
    cdx_max_interval: Duration,

    /// Content pacing bounds and initial interval.
    content_min_interval: Duration,
    content_initial_interval: Duration,
    content_max_interval: Duration,

    /// Cooldown durations (hysteresis) applied after various backpressure signals.
    cooldown_on_429: Duration,
    cooldown_on_5xx: Duration,
    cooldown_on_decode: Duration,
    cooldown_on_timeout: Duration,
    cooldown_on_other: Duration,
}

impl AdaptiveConfig {
    fn default() -> Self {
        Self {
            slow_start: true,
            slow_start_threshold: Duration::from_secs(1),
            slow_start_divisor: 2,

            // Recover very slowly (prevents flapping).
            success_step: Duration::from_millis(50),
            // Back off quickly when we see backpressure.
            backoff_factor: 2,

            // CDX is the most fragile surface (documented limits and escalating penalties).
            cdx_min_interval: Duration::from_millis(1200),
            cdx_initial_interval: Duration::from_millis(1500),
            cdx_max_interval: Duration::from_secs(30),

            // Content is separate; still conservative by default.
            content_min_interval: Duration::from_millis(800),
            content_initial_interval: Duration::from_millis(1500),
            content_max_interval: Duration::from_secs(20),

            // Hysteresis windows: hold slower rates for a while after backpressure.
            cooldown_on_429: Duration::from_secs(60 * 10),
            cooldown_on_5xx: Duration::from_secs(60),
            cooldown_on_decode: Duration::from_secs(30),
            cooldown_on_timeout: Duration::from_secs(10),
            cooldown_on_other: Duration::from_secs(10),
        }
    }
}

/// All pacing knobs for a given profile, in one place.
///
/// This keeps strategy selection out of the controller logic and makes it easy
/// to tune behavior without scattering magic numbers.
#[derive(Clone, Copy, Debug)]
struct PacingProfileConfig {
    static_cfg: StaticPacingConfig,
    adaptive_cfg: AdaptiveConfig,
}

impl PacingProfileConfig {
    /// Baseline defaults. Other profiles should start here and override.
    fn default() -> Self {
        Self {
            static_cfg: StaticPacingConfig {
                cdx_interval: Duration::from_secs(1),
                cdx_burst: 5,
                content_interval: Duration::from_millis(1500),
                content_burst: 5,
            },
            adaptive_cfg: AdaptiveConfig::default(),
        }
    }

    fn for_profile(profile: WaybackPacingProfile) -> Self {
        let mut cfg = Self::default();

        match profile {
            WaybackPacingProfile::Default => cfg,
            WaybackPacingProfile::Conservative => {
                // Conservative: reduce burstiness and slow both surfaces a bit.
                cfg.static_cfg.cdx_interval = Duration::from_millis(1500);
                cfg.static_cfg.cdx_burst = 2;
                cfg.static_cfg.content_interval = Duration::from_millis(2000);
                cfg.static_cfg.content_burst = 2;

                // Conservative adaptive: slower recovery and longer cooldowns.
                // Use a gentler slow-start ramp (interval shrinks by ~25% per success).
                cfg.adaptive_cfg.slow_start_divisor = 4;
                cfg.adaptive_cfg.success_step = Duration::from_millis(25);
                cfg.adaptive_cfg.cdx_min_interval = Duration::from_millis(1500);
                cfg.adaptive_cfg.cdx_initial_interval = Duration::from_millis(2000);
                cfg.adaptive_cfg.cooldown_on_429 = Duration::from_secs(60 * 15);
                cfg
            }
            WaybackPacingProfile::Adaptive => {
                // Adaptive profile uses the adaptive controller; static values are still
                // defined for completeness / future use.
                cfg
            }
        }
    }
}

/// Create an opt-in `wayback_rs::Pacer` for cancel-culture.
///
/// This provides separate pacing hooks for the CDX API surface and for content
/// downloads, while remaining a small, self-contained change.
pub fn wayback_pacer(profile: WaybackPacingProfile) -> Arc<wayback_rs::Pacer> {
    let cfg = PacingProfileConfig::for_profile(profile);

    if matches!(profile, WaybackPacingProfile::Adaptive) {
        return adaptive_wayback_pacer_with_cfg(cfg.adaptive_cfg).pacer;
    }

    let StaticPacingConfig {
        cdx_interval,
        cdx_burst,
        content_interval,
        content_burst,
    } = cfg.static_cfg;

    let cdx = Arc::new(
        leaky_bucket::RateLimiter::builder()
            .max(cdx_burst)
            .initial(cdx_burst)
            .interval(cdx_interval)
            .build(),
    );

    let content = Arc::new(
        leaky_bucket::RateLimiter::builder()
            .max(content_burst)
            .initial(content_burst)
            .interval(content_interval)
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

pub fn default_wayback_pacer() -> Arc<wayback_rs::Pacer> {
    wayback_pacer(WaybackPacingProfile::Default)
}

/// Adaptive controller that produces a `Pacer` plus an `Observer`.
///
/// The observer updates shared state based on request outcomes, and the pacer
/// consults that state before sending requests.
pub struct AdaptiveWayback {
    pub pacer: Arc<wayback_rs::Pacer>,
    pub observer: Arc<dyn wayback_rs::Observer>,
}

#[derive(Debug)]
struct SurfaceState {
    interval: Duration,
    min_interval: Duration,
    max_interval: Duration,
    next_allowed: Instant,
    cooldown_until: Instant,
    in_slow_start: bool,
}

impl SurfaceState {
    fn new(min_interval: Duration, initial: Duration, max_interval: Duration) -> Self {
        let now = Instant::now();
        Self {
            interval: initial,
            min_interval,
            max_interval,
            next_allowed: now,
            cooldown_until: now,
            in_slow_start: true,
        }
    }

    fn on_success(&mut self, cfg: AdaptiveConfig) {
        let now = Instant::now();
        if now < self.cooldown_until {
            return;
        }

        if cfg.slow_start && self.in_slow_start && self.interval > cfg.slow_start_threshold {
            // Slow start: ramp up quickly by shrinking the interval multiplicatively.
            // For conservative tuning we want less-steep growth; use a rational
            // approximation when divisor > 2.
            self.interval = if cfg.slow_start_divisor <= 2 {
                (self.interval / cfg.slow_start_divisor).max(self.min_interval)
            } else {
                // interval *= (divisor-1)/divisor, e.g. 3/4 each success
                let d = cfg.slow_start_divisor as u128;
                let n = (cfg.slow_start_divisor - 1) as u128;
                let nanos = self.interval.as_nanos();
                let next = (nanos * n) / d;
                Duration::from_nanos(next.min(u64::MAX as u128) as u64).max(self.min_interval)
            };

            if self.interval <= cfg.slow_start_threshold {
                self.in_slow_start = false;
            }
            return;
        }

        // Additive recovery: reduce interval in small steps.
        self.interval = self.interval.saturating_sub(cfg.success_step).max(self.min_interval);
    }

    fn on_backpressure(&mut self, cfg: AdaptiveConfig, cooldown: Duration) {
        let now = Instant::now();
        // Fast backoff: multiplicative increase, then hold for a while (hysteresis).
        self.interval = (self.interval * cfg.backoff_factor).min(self.max_interval);
        self.cooldown_until = now + cooldown;
        // After congestion/backpressure, re-enter slow start so we recover quickly
        // up to the configured threshold, then switch to additive recovery.
        self.in_slow_start = cfg.slow_start;
    }

    fn acquire_delay(&mut self) -> Duration {
        let now = Instant::now();
        let mut target = self.next_allowed;
        if now < self.cooldown_until {
            target = target.max(self.cooldown_until);
        }
        if target < now {
            target = now;
        }
        self.next_allowed = target + self.interval;
        target.saturating_duration_since(now)
    }
}

#[derive(Debug)]
struct AdaptiveControllerInner {
    cfg: AdaptiveConfig,
    cdx: Mutex<SurfaceState>,
    content: Mutex<SurfaceState>,
}

impl AdaptiveControllerInner {
    fn new(cfg: AdaptiveConfig) -> Arc<Self> {
        let cdx = SurfaceState::new(cfg.cdx_min_interval, cfg.cdx_initial_interval, cfg.cdx_max_interval);
        let content = SurfaceState::new(
            cfg.content_min_interval,
            cfg.content_initial_interval,
            cfg.content_max_interval,
        );
        Arc::new(Self {
            cfg,
            cdx: Mutex::new(cdx),
            content: Mutex::new(content),
        })
    }

    async fn pace_cdx(&self) {
        let delay = {
            let mut st = self.cdx.lock().expect("cdx state poisoned");
            st.acquire_delay()
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }

    async fn pace_content(&self) {
        let delay = {
            let mut st = self.content.lock().expect("content state poisoned");
            st.acquire_delay()
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }

    fn apply_event(&self, event: &wayback_rs::Event) {
        use wayback_rs::{ErrorClass, Phase, Surface};

        let is_success = matches!(event.phase, Phase::Complete)
            && event.status == Some(200);

        let state = match event.surface {
            Surface::Cdx => &self.cdx,
            Surface::Content => &self.content,
            _ => return,
        };

        if is_success {
            if let Ok(mut st) = state.lock() {
                st.on_success(self.cfg);
            }
            return;
        }

        if !matches!(event.phase, Phase::Error) {
            return;
        }

        // Backpressure heuristics with hysteresis:
        // - 429: long cooldown (avoid escalating penalties)
        // - 5xx: medium cooldown
        // - decode/non-JSON: medium cooldown (often signals edge errors)
        // - timeouts/connect: shorter cooldown
        let cooldown = match event.status {
            Some(429) => self.cfg.cooldown_on_429,
            Some(s) if s >= 500 => self.cfg.cooldown_on_5xx,
            Some(_) => self.cfg.cooldown_on_other,
            None => match event.error {
                Some(ErrorClass::Timeout) | Some(ErrorClass::Connect) => self.cfg.cooldown_on_timeout,
                Some(ErrorClass::Decode) => self.cfg.cooldown_on_decode,
                _ => self.cfg.cooldown_on_other,
            },
        };

        if let Ok(mut st) = state.lock() {
            st.on_backpressure(self.cfg, cooldown);
        }

    }
}

struct AdaptiveObserver {
    inner: Arc<AdaptiveControllerInner>,
}

impl wayback_rs::Observer for AdaptiveObserver {
    fn on_event(&self, event: &wayback_rs::Event) {
        self.inner.apply_event(event);
    }
}

pub fn adaptive_wayback_pacer() -> AdaptiveWayback {
    adaptive_wayback_pacer_with_cfg(AdaptiveConfig::default())
}

pub fn adaptive_wayback_pacer_with_cfg(cfg: AdaptiveConfig) -> AdaptiveWayback {
    let inner = AdaptiveControllerInner::new(cfg);
    let observer: Arc<dyn wayback_rs::Observer> = Arc::new(AdaptiveObserver { inner: inner.clone() });

    let pacer = Arc::new(wayback_rs::Pacer::new(
        {
            let inner = inner.clone();
            move || {
                let inner = inner.clone();
                async move { inner.pace_cdx().await }
            }
        },
        {
            let inner = inner.clone();
            move || {
                let inner = inner.clone();
                async move { inner.pace_content().await }
            }
        },
    ));

    AdaptiveWayback { pacer, observer }
}


