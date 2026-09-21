use std::time::Duration;

use crate::change::ChangeReport;

/// How aggressively the pipeline is allowed to sample the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Responsiveness {
    Fast,
    Balanced,
    Accurate,
}

impl Responsiveness {
    const fn ceiling_hz(self) -> u32 {
        match self {
            Responsiveness::Fast => 30,
            Responsiveness::Balanced => 15,
            Responsiveness::Accurate => 8,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SchedulerConfig {
    pub responsiveness: Responsiveness,
    /// Rate the scheduler settles at once the screen stops changing.
    pub idle_hz: u32,
    /// Fraction of tiles that must change for a frame to count as active.
    pub activity_threshold: f32,
    /// Consecutive static frames tolerated before the rate is halved.
    pub calm_frames: u32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            responsiveness: Responsiveness::Balanced,
            idle_hz: 2,
            activity_threshold: 0.002,
            calm_frames: 4,
        }
    }
}

/// Chooses the delay before the next capture.
///
/// Two pressures act on the rate. Text that is changing pulls it up to the ceiling so a new menu
/// appears translated quickly; a screen that has been still pulls it down so an idle desktop costs
/// almost nothing. A host application that is dropping frames overrides both: the translator must
/// never be the reason a game stutters.
#[derive(Debug, Clone)]
pub struct CaptureScheduler {
    config: SchedulerConfig,
    current_hz: f32,
    calm_streak: u32,
    throttled: bool,
}

impl CaptureScheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            current_hz: config.idle_hz.max(1) as f32,
            config,
            calm_streak: 0,
            throttled: false,
        }
    }

    pub fn current_hz(&self) -> f32 {
        self.current_hz
    }

    pub fn is_throttled(&self) -> bool {
        self.throttled
    }

    /// Reports the host application's frame health. Anything below 0.9 of its own target means the
    /// host is struggling and the pipeline gives the hardware back.
    pub fn observe_host_frame_health(&mut self, ratio: f32) {
        self.throttled = ratio < 0.9;
        if self.throttled {
            self.current_hz = (self.current_hz * 0.5).max(1.0);
        }
    }

    pub fn next_delay(&mut self, report: &ChangeReport) -> Duration {
        let ceiling = self.config.responsiveness.ceiling_hz() as f32;
        let floor = self.config.idle_hz.max(1) as f32;
        let ceiling = if self.throttled { (ceiling * 0.25).max(floor) } else { ceiling };

        if report.changed_fraction() >= self.config.activity_threshold {
            self.calm_streak = 0;
            self.current_hz = (self.current_hz * 1.8).clamp(floor, ceiling);
        } else {
            self.calm_streak += 1;
            if self.calm_streak >= self.config.calm_frames {
                self.calm_streak = 0;
                self.current_hz = (self.current_hz * 0.5).max(floor);
            }
        }

        Duration::from_secs_f32(1.0 / self.current_hz.clamp(floor, ceiling))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;

    fn report(changed: usize, total: usize) -> ChangeReport {
        ChangeReport {
            regions: vec![Rect::new(0, 0, 64, 64); changed.min(1)],
            changed_tiles: changed,
            total_tiles: total,
            reset: false,
        }
    }

    #[test]
    fn activity_raises_the_rate_towards_the_ceiling() {
        let mut scheduler = CaptureScheduler::new(SchedulerConfig::default());
        for _ in 0..8 {
            scheduler.next_delay(&report(40, 100));
        }
        assert_eq!(scheduler.current_hz(), 15.0);
    }

    #[test]
    fn a_still_screen_settles_at_the_idle_rate() {
        let mut scheduler = CaptureScheduler::new(SchedulerConfig::default());
        for _ in 0..8 {
            scheduler.next_delay(&report(40, 100));
        }
        for _ in 0..40 {
            scheduler.next_delay(&report(0, 100));
        }
        assert_eq!(scheduler.current_hz(), 2.0);
    }

    #[test]
    fn the_rate_never_falls_below_the_idle_floor() {
        let mut scheduler = CaptureScheduler::new(SchedulerConfig::default());
        for _ in 0..100 {
            scheduler.next_delay(&report(0, 100));
        }
        assert!(scheduler.current_hz() >= 2.0);
    }

    #[test]
    fn a_struggling_host_caps_the_rate() {
        let mut scheduler = CaptureScheduler::new(SchedulerConfig {
            responsiveness: Responsiveness::Fast,
            ..SchedulerConfig::default()
        });
        for _ in 0..8 {
            scheduler.next_delay(&report(40, 100));
        }
        scheduler.observe_host_frame_health(0.6);
        for _ in 0..4 {
            scheduler.next_delay(&report(40, 100));
        }
        assert!(scheduler.is_throttled());
        assert!(scheduler.current_hz() <= 7.5);
    }

    #[test]
    fn host_recovery_lifts_the_cap_again() {
        let mut scheduler = CaptureScheduler::new(SchedulerConfig::default());
        scheduler.observe_host_frame_health(0.5);
        scheduler.observe_host_frame_health(1.0);
        assert!(!scheduler.is_throttled());
    }

    #[test]
    fn delays_match_the_chosen_rate() {
        let mut scheduler = CaptureScheduler::new(SchedulerConfig::default());
        let delay = scheduler.next_delay(&report(0, 100));
        assert!((delay.as_secs_f32() - 0.5).abs() < 1e-3);
    }
}
