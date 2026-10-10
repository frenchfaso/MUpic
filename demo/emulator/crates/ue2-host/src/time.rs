//! Host wall clocks. Emulated clocks remain in the machine core.
#[cfg(any(test, all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::Duration;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[link(wasm_import_module = "ue2_host")]
extern "C" {
    #[link_name = "utc_ms"]
    fn browser_utc_ms() -> f64;
    #[link_name = "monotonic_ms"]
    fn browser_monotonic_ms() -> f64;
}

/// Milliseconds since the Unix epoch; the browser supplies `Date.now()`.
pub fn utc_ms() -> f64 {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    { unsafe { browser_utc_ms() } }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64() * 1000.0) }
}

/// UTC whole seconds, preserving the RTC's before-epoch fallback.
pub fn utc_secs() -> i64 {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let ms = utc_ms();
        if ms.is_finite() && ms > 0.0 { (ms / 1000.0).floor() as i64 } else { 0 }
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64) }
}

/// Monotonic milliseconds; the browser supplies `performance.now()`.
pub fn monotonic_ms() -> f64 {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    { unsafe { browser_monotonic_ms() } }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        EPOCH.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use std::time::Instant;

/// A timestamp within this WASM instance's monotonic host clock.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Instant(f64);

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl Instant {
    pub fn now() -> Self { Self(monotonic_ms()) }

    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        let seconds = ((self.0 - earlier.0) / 1000.0).max(0.0);
        Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
    }

    pub fn elapsed(self) -> Duration { Self::now().saturating_duration_since(self) }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl std::ops::Add<Duration> for Instant {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self { Self(self.0 + rhs.as_secs_f64() * 1000.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_utc_and_instant_preserve_clock_domains() {
        let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let utc = utc_secs();
        let after = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!((before..=after).contains(&utc));
        let instant = Instant::now();
        assert_eq!((instant + Duration::from_millis(500)).saturating_duration_since(instant), Duration::from_millis(500));
        assert_eq!(instant.saturating_duration_since(instant + Duration::from_millis(1)), Duration::ZERO);
        assert!(monotonic_ms().is_finite());
    }
}
