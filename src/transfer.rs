//! Transfer measurements use a monotonic clock and decimal megabytes.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Rate {
    samples: VecDeque<(Duration, u64)>,
}
impl Rate {
    pub fn sample(&mut self, elapsed: Duration, received: u64) -> Option<u64> {
        if self
            .samples
            .back()
            .is_some_and(|(time, bytes)| elapsed < *time || received < *bytes)
        {
            self.samples.clear();
        }
        if self.samples.is_empty() {
            self.samples.push_back((Duration::ZERO, 0));
        }
        self.samples.push_back((elapsed, received));
        let cutoff = elapsed.saturating_sub(Duration::from_secs(2));
        while self.samples.len() > 2 && self.samples[1].0 <= cutoff {
            self.samples.pop_front();
        }
        let (start, bytes) = self.samples.front().copied()?;
        let seconds = elapsed.saturating_sub(start).as_secs_f64();
        (seconds >= 0.05).then(|| ((received - bytes) as f64 / seconds) as u64)
    }
}

pub fn label(rate: Option<u64>) -> String {
    rate.map_or_else(
        || "Calculating…".into(),
        |bytes| format!("{:.1} MB/s", bytes as f64 / 1_000_000.0),
    )
}

#[derive(Clone)]
pub struct DownloadProgress {
    pub name: String,
    pub received: u64,
    pub total: Option<u64>,
    pub bytes_per_second: Option<u64>,
    pub updated: Instant,
}
impl DownloadProgress {
    pub fn new(
        name: String,
        received: u64,
        total: Option<u64>,
        bytes_per_second: Option<u64>,
    ) -> Self {
        Self {
            name,
            received,
            total,
            bytes_per_second,
            updated: Instant::now(),
        }
    }
    pub fn rate_label(&self) -> String {
        label(if self.updated.elapsed() >= Duration::from_secs(2) {
            Some(0)
        } else {
            self.bytes_per_second
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rolling_rate_tracks_changes_and_resets() {
        let mut rate = Rate::default();
        assert_eq!(rate.sample(Duration::ZERO, 0), None);
        assert_eq!(
            rate.sample(Duration::from_secs(1), 1_000_000),
            Some(1_000_000)
        );
        assert_eq!(
            rate.sample(Duration::from_secs(2), 2_000_000),
            Some(1_000_000)
        );
        assert_eq!(
            rate.sample(Duration::from_secs(3), 5_000_000),
            Some(2_000_000)
        );
        assert_eq!(rate.sample(Duration::from_secs(5), 5_000_000), Some(0));
        assert_eq!(rate.sample(Duration::from_secs(1), 500_000), Some(500_000));
        assert_eq!(label(Some(12_340_000)), "12.3 MB/s");
    }
    #[test]
    fn silent_download_drops_to_zero() {
        let mut progress = DownloadProgress::new("test".into(), 1, None, Some(1_000_000));
        progress.updated -= Duration::from_secs(3);
        assert_eq!(progress.rate_label(), "0.0 MB/s");
    }
}
