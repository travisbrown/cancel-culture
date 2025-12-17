# Changelog (fork)

This fork tracks `travisbrown/cancel-culture`.

## Unreleased

### Added
- `twcc deleted-tweets --no-api` to allow running without Twitter API credentials (skips API availability checks).

## fork/cancel-culture/v0.1.0-chad.2 (based on upstream `main` @ 88f8fef)

### Added
- `--wayback-pacing` runtime option to control Wayback request pacing strategy (static and adaptive profiles).
- Adaptive pacing controller with separate per-surface control (CDX vs content) using synchronous request events from `wayback-rs`.
- On-demand pacing stats dump for long runs: send SIGUSR1 (and SIGINFO / Ctrl+T on macOS) to print the current scoreboard/state.

### Changed
- `wayback-rs` dependency is pinned to a fork tag for reproducible tester builds.

