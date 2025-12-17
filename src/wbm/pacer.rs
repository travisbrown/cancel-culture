use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
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

    /// Maximum cooldown we will ever apply (upper bound on "jail time").
    ///
    /// This is a safety valve for hostile environments where penalties may extend
    /// to hours. We allow runtime override via env vars so the tool can adapt
    /// without rebuilding.
    max_cooldown: Duration,

    /// Multiplier applied to the base cooldown on repeated backpressure events.
    ///
    /// Effective cooldown is scaled as `base * (cooldown_growth ^ penalty_level)`
    /// (bounded by `max_cooldown`).
    cooldown_growth: u32,

    /// Maximum number of escalation steps.
    max_penalty_level: u32,
}

impl AdaptiveConfig {
    fn env_u64(var: &str) -> Option<u64> {
        std::env::var(var).ok().and_then(|s| s.trim().parse::<u64>().ok())
    }

    fn env_u32(var: &str) -> Option<u32> {
        std::env::var(var).ok().and_then(|s| s.trim().parse::<u32>().ok())
    }

    fn apply_env_overrides(&mut self) {
        // All overrides are optional and intentionally low-noise.
        //
        // Values are in seconds (simple to set) and affect *adaptive* pacing only.
        if let Some(v) = Self::env_u64("CANCEL_CULTURE_WAYBACK_COOLDOWN_429_SECS") {
            self.cooldown_on_429 = Duration::from_secs(v);
        }
        if let Some(v) = Self::env_u64("CANCEL_CULTURE_WAYBACK_COOLDOWN_5XX_SECS") {
            self.cooldown_on_5xx = Duration::from_secs(v);
        }
        if let Some(v) = Self::env_u64("CANCEL_CULTURE_WAYBACK_COOLDOWN_DECODE_SECS") {
            self.cooldown_on_decode = Duration::from_secs(v);
        }
        if let Some(v) = Self::env_u64("CANCEL_CULTURE_WAYBACK_COOLDOWN_TIMEOUT_SECS") {
            self.cooldown_on_timeout = Duration::from_secs(v);
        }
        if let Some(v) = Self::env_u64("CANCEL_CULTURE_WAYBACK_COOLDOWN_OTHER_SECS") {
            self.cooldown_on_other = Duration::from_secs(v);
        }
        if let Some(v) = Self::env_u64("CANCEL_CULTURE_WAYBACK_MAX_COOLDOWN_SECS") {
            self.max_cooldown = Duration::from_secs(v);
        }
        if let Some(v) = Self::env_u32("CANCEL_CULTURE_WAYBACK_COOLDOWN_GROWTH") {
            self.cooldown_growth = v.max(1);
        }
        if let Some(v) = Self::env_u32("CANCEL_CULTURE_WAYBACK_MAX_PENALTY_LEVEL") {
            self.max_penalty_level = v;
        }
    }

    fn default() -> Self {
        let mut cfg = Self {
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

            // Default upper bound: allow "1 hour jail time" by default, but keep it configurable.
            max_cooldown: Duration::from_secs(60 * 60),
            // Default escalation is exponential (2x) with a modest cap on steps.
            cooldown_growth: 2,
            max_penalty_level: 6, // up to 10m * 2^6 ~= 10h, then capped by max_cooldown
        };
        cfg.apply_env_overrides();
        cfg
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
        };

        // Finally, allow runtime overrides regardless of profile.
        cfg.adaptive_cfg.apply_env_overrides();
        cfg
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
    pub stats: AdaptiveStats,
}

#[derive(Clone)]
pub struct AdaptiveStats {
    inner: Arc<AdaptiveControllerInner>,
}

impl AdaptiveStats {
    pub fn snapshot(&self) -> AdaptiveSnapshot {
        self.inner.snapshot()
    }

    pub fn format(&self) -> String {
        self.snapshot().format()
    }
}

#[derive(Clone, Debug)]
pub struct SurfaceSnapshot {
    pub interval: Duration,
    pub min_interval: Duration,
    pub max_interval: Duration,
    pub cooldown_remaining: Duration,
    pub slow_start: bool,
}

#[derive(Clone, Debug)]
pub struct AdaptiveSnapshot {
    pub cdx: SurfaceSnapshot,
    pub content: SurfaceSnapshot,
    pub success: u64,
    pub errors: u64,
    pub errors_429: u64,
    pub errors_5xx: u64,
    pub errors_decode: u64,
    pub errors_timeout: u64,
    pub errors_blocked: u64,
}

impl AdaptiveSnapshot {
    pub fn format(&self) -> String {
        fn ms(d: Duration) -> u128 {
            d.as_millis()
        }
        fn fmt_surface(name: &str, s: &SurfaceSnapshot) -> String {
            format!(
                "{name:7} interval={:>5}ms (min={:>5}ms max={:>5}ms) cooldown={:>5}ms slow_start={}",
                ms(s.interval),
                ms(s.min_interval),
                ms(s.max_interval),
                ms(s.cooldown_remaining),
                s.slow_start
            )
        }

        let mut out = String::new();
        out.push_str("Wayback pacing stats (adaptive)\n");
        out.push_str(&fmt_surface("CDX", &self.cdx));
        out.push('\n');
        out.push_str(&fmt_surface("Content", &self.content));
        out.push('\n');
        out.push_str(&format!(
            "events: success={} errors={} (429={} 5xx={} decode={} timeout={} blocked={})",
            self.success,
            self.errors,
            self.errors_429,
            self.errors_5xx,
            self.errors_decode,
            self.errors_timeout,
            self.errors_blocked
        ));
        out
    }
}

#[derive(Debug)]
struct SurfaceState {
    interval: Duration,
    min_interval: Duration,
    max_interval: Duration,
    next_allowed: Instant,
    cooldown_until: Instant,
    in_slow_start: bool,
    penalty_level: u32,
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
            penalty_level: 0,
        }
    }

    fn on_success(&mut self, cfg: AdaptiveConfig) {
        let now = Instant::now();
        if now < self.cooldown_until {
            return;
        }

        // Successful completion outside cooldown slowly reduces our "penalty level".
        // This makes long jail times recover gracefully rather than snapping back.
        if self.penalty_level > 0 {
            self.penalty_level -= 1;
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
        self.penalty_level = (self.penalty_level + 1).min(cfg.max_penalty_level);

        fn scaled_cooldown(base: Duration, growth: u32, steps: u32, cap: Duration) -> Duration {
            if base.is_zero() || growth <= 1 || steps == 0 {
                return base.min(cap);
            }
            // Multiply base by growth^steps with saturation.
            let mut mult: u128 = 1;
            for _ in 0..steps {
                mult = mult.saturating_mul(growth as u128);
            }
            let nanos = base.as_nanos().saturating_mul(mult);
            let scaled = Duration::from_nanos(nanos.min(u64::MAX as u128) as u64);
            scaled.min(cap)
        }

        let effective_cooldown =
            scaled_cooldown(cooldown, cfg.cooldown_growth, self.penalty_level, cfg.max_cooldown);
        self.cooldown_until = now + effective_cooldown;
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
    success: AtomicU64,
    errors: AtomicU64,
    errors_429: AtomicU64,
    errors_5xx: AtomicU64,
    errors_decode: AtomicU64,
    errors_timeout: AtomicU64,
    errors_blocked: AtomicU64,
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
            success: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            errors_429: AtomicU64::new(0),
            errors_5xx: AtomicU64::new(0),
            errors_decode: AtomicU64::new(0),
            errors_timeout: AtomicU64::new(0),
            errors_blocked: AtomicU64::new(0),
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
            self.success.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut st) = state.lock() {
                st.on_success(self.cfg);
            }
            return;
        }

        if !matches!(event.phase, Phase::Error) {
            return;
        }

        self.errors.fetch_add(1, Ordering::Relaxed);

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
                Some(ErrorClass::Blocked) => self.cfg.cooldown_on_429,
                _ => self.cfg.cooldown_on_other,
            },
        };

        match event.status {
            Some(429) => {
                self.errors_429.fetch_add(1, Ordering::Relaxed);
            }
            Some(s) if s >= 500 => {
                self.errors_5xx.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        if matches!(event.error, Some(ErrorClass::Decode)) {
            self.errors_decode.fetch_add(1, Ordering::Relaxed);
        }
        if matches!(event.error, Some(ErrorClass::Timeout) | Some(ErrorClass::Connect)) {
            self.errors_timeout.fetch_add(1, Ordering::Relaxed);
        }
        if matches!(event.error, Some(ErrorClass::Blocked)) {
            self.errors_blocked.fetch_add(1, Ordering::Relaxed);
        }

        if let Ok(mut st) = state.lock() {
            st.on_backpressure(self.cfg, cooldown);
        }

    }

    fn snapshot(&self) -> AdaptiveSnapshot {
        fn snap_surface(m: &Mutex<SurfaceState>) -> SurfaceSnapshot {
            let now = Instant::now();
            if let Ok(st) = m.lock() {
                SurfaceSnapshot {
                    interval: st.interval,
                    min_interval: st.min_interval,
                    max_interval: st.max_interval,
                    cooldown_remaining: st
                        .cooldown_until
                        .saturating_duration_since(now),
                    slow_start: st.in_slow_start,
                }
            } else {
                // If poisoned, return a safe empty snapshot.
                SurfaceSnapshot {
                    interval: Duration::from_secs(0),
                    min_interval: Duration::from_secs(0),
                    max_interval: Duration::from_secs(0),
                    cooldown_remaining: Duration::from_secs(0),
                    slow_start: false,
                }
            }
        }

        AdaptiveSnapshot {
            cdx: snap_surface(&self.cdx),
            content: snap_surface(&self.content),
            success: self.success.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            errors_429: self.errors_429.load(Ordering::Relaxed),
            errors_5xx: self.errors_5xx.load(Ordering::Relaxed),
            errors_decode: self.errors_decode.load(Ordering::Relaxed),
            errors_timeout: self.errors_timeout.load(Ordering::Relaxed),
            errors_blocked: self.errors_blocked.load(Ordering::Relaxed),
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

fn adaptive_wayback_pacer_with_cfg(cfg: AdaptiveConfig) -> AdaptiveWayback {
    let inner = AdaptiveControllerInner::new(cfg);
    let observer: Arc<dyn wayback_rs::Observer> = Arc::new(AdaptiveObserver { inner: inner.clone() });
    let stats = AdaptiveStats { inner: inner.clone() };

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

    AdaptiveWayback { pacer, observer, stats }
}


