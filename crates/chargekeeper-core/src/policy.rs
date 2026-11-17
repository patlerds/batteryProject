use crate::{Capabilities, Snapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Limit,
    TopUp,
    Calibration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema: u32,
    pub limit: u8,
    pub band: u8,
    pub mode: Mode,
    pub heat_c: Option<u8>,
    pub discharge: bool,
    pub notifications: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema: 1,
            limit: 80,
            band: 5,
            mode: Mode::Limit,
            heat_c: None,
            discharge: false,
            notifications: true,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("Unsupported configuration version".into());
        }
        if !(50..=100).contains(&self.limit) || !(2..=20).contains(&self.band) {
            return Err("Limit must be 50–100%; sailing band must be 2–20%".into());
        }
        if self.heat_c.is_some_and(|t| !(35..=50).contains(&t)) {
            return Err("Heat threshold must be 35–50 °C".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Limit,
    TopUp,
    FullHold,
    Discharging,
    Recharging,
    Complete,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub phase: Phase,
    pub inhibit: bool,
    pub discharge: bool,
    pub target: u8,
    pub restore: bool,
    pub reminder: bool,
    pub reason: &'static str,
}

pub struct Engine {
    phase: Phase,
    mode: Mode,
    holding: bool,
    hot: bool,
    hold_since: Option<u64>,
    last_time: Option<u64>,
    reminded: bool,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            phase: Phase::Limit,
            mode: Mode::Limit,
            holding: false,
            hot: false,
            hold_since: None,
            last_time: None,
            reminded: false,
        }
    }
}
impl Engine {
    fn cancel_temporary_mode(&mut self) {
        if self.mode != Mode::Limit {
            self.phase = Phase::Complete;
        }
        self.hold_since = None;
    }
    /// `now` is monotonic seconds, never wall-clock or a time zone dependent value.
    pub fn step(&mut self, c: &Config, s: &Snapshot, caps: Capabilities, now: u64) -> Decision {
        let mut d = Decision {
            phase: self.phase,
            inhibit: false,
            discharge: false,
            target: c.limit,
            restore: false,
            reminder: false,
            reason: "Monitoring",
        };
        let backwards = self
            .last_time
            .is_some_and(|last| now < last || now - last > 30);
        self.last_time = Some(now);
        if c.validate().is_err() || !s.valid() || backwards {
            self.holding = false;
            self.hot = false;
            self.hold_since = None;
            self.cancel_temporary_mode();
            d.phase = self.phase;
            d.restore = true;
            d.reason = "Telemetry unavailable or resumed; restore safe defaults";
            return d;
        }
        let low = s.lowest().unwrap_or(0.0);
        let high = s.highest().unwrap_or(0.0);
        if self.mode != c.mode {
            self.mode = c.mode;
            self.hold_since = None;
            self.holding = false;
            self.phase = match c.mode {
                Mode::Limit => Phase::Limit,
                Mode::TopUp => Phase::TopUp,
                Mode::Calibration => Phase::TopUp,
            };
        }
        if low <= 20.0 {
            self.holding = false;
            self.hot = false;
            if self.phase == Phase::Discharging {
                self.phase = Phase::Recharging;
            }
            d.restore = true;
            d.phase = self.phase;
            d.reason = "Safety floor: normal charging restored";
            return d;
        }
        if s.ac != Some(true) {
            self.holding = false;
            self.hot = false;
            self.hold_since = None;
            self.cancel_temporary_mode();
            d.phase = self.phase;
            d.restore = true;
            d.reason = "AC power unavailable";
            return d;
        }
        if c.mode == Mode::Calibration && !(caps.inhibit && caps.force_discharge) {
            d.restore = true;
            d.reason = "Calibration unsupported by provider";
            return d;
        }
        if c.mode == Mode::TopUp && !(caps.set_limit || caps.inhibit) {
            d.restore = true;
            d.reason = "Top-up unsupported by provider";
            return d;
        }
        if c.heat_c.is_some() && !(caps.temperature && caps.inhibit) {
            d.restore = true;
            d.reason = "Heat protection unsupported by provider";
            return d;
        }
        if let Some(threshold) = c.heat_c {
            if s.batteries.iter().any(|b| b.temperature_c.is_none()) {
                d.restore = true;
                d.reason = "Battery temperature unavailable";
                return d;
            }
            let hottest = s
                .batteries
                .iter()
                .filter_map(|b| b.temperature_c)
                .fold(f64::NEG_INFINITY, f64::max);
            if hottest >= f64::from(threshold) {
                self.hot = true;
            }
            if hottest <= f64::from(threshold) - 3.0 {
                self.hot = false;
            }
        } else {
            self.hot = false;
        }
        if self.hot {
            d.inhibit = true;
            d.reason = "Heat protection paused charging";
            return d;
        }
        match self.phase {
            Phase::TopUp if low >= 100.0 => {
                if c.mode == Mode::Calibration {
                    self.phase = Phase::FullHold;
                    self.hold_since = Some(now);
                } else {
                    self.phase = Phase::Complete;
                }
            }
            Phase::FullHold if now.saturating_sub(self.hold_since.unwrap_or(now)) >= 600 => {
                self.phase = Phase::Discharging
            }
            Phase::Discharging if low <= 25.0 => self.phase = Phase::Recharging,
            Phase::Recharging if low >= 100.0 => self.phase = Phase::Complete,
            _ => {}
        }
        let full = matches!(self.phase, Phase::TopUp | Phase::Recharging);
        d.phase = self.phase;
        d.target = if full { 100 } else { c.limit };
        if high >= f64::from(d.target) {
            self.holding = true;
        }
        if high <= f64::from(d.target.saturating_sub(c.band)) {
            self.holding = false;
            self.reminded = false;
        }
        d.inhibit = caps.inhibit
            && (self.holding || matches!(self.phase, Phase::FullHold | Phase::Discharging));
        d.discharge = caps.force_discharge
            && (self.phase == Phase::Discharging
                || (c.discharge && !full && high > f64::from(c.limit)));
        d.reason = if caps.set_limit || caps.inhibit {
            "Policy ready; enforcement requires controller readback"
        } else {
            "Software reminders only; Windows cannot enforce this limit"
        };
        if !caps.set_limit
            && !caps.inhibit
            && high >= f64::from(c.limit)
            && !self.reminded
            && c.notifications
        {
            self.reminded = true;
            d.reminder = true;
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Battery;
    fn sample(p: f64, t: Option<f64>) -> Snapshot {
        Snapshot {
            batteries: vec![Battery {
                percent: p,
                state: "charging".into(),
                power_w: None,
                temperature_c: t,
                cycles: None,
                health: None,
            }],
            ac: Some(true),
            fresh: true,
        }
    }
    fn caps() -> Capabilities {
        Capabilities {
            set_limit: true,
            thresholds: true,
            inhibit: true,
            force_discharge: true,
            temperature: true,
        }
    }
    #[test]
    fn sailing_hysteresis() {
        let mut e = Engine::default();
        let c = Config::default();
        for (i, (p, hold)) in [
            (79., false),
            (80., true),
            (79., true),
            (76., true),
            (75., false),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(e.step(&c, &sample(p, None), caps(), i as u64).inhibit, hold);
        }
    }
    #[test]
    fn fallback_is_honest_and_reminders_do_not_spam() {
        let mut e = Engine::default();
        let c = Config::default();
        let d = e.step(&c, &sample(85., None), Capabilities::default(), 0);
        assert!(d.reminder);
        assert!(!d.inhibit);
        assert!(!d.discharge);
        assert!(
            !e.step(&c, &sample(85., None), Capabilities::default(), 10)
                .reminder
        );
    }
    #[test]
    fn safety_floor_overrides_discharge() {
        let mut e = Engine::default();
        let c = Config {
            discharge: true,
            ..Config::default()
        };
        let mut s = sample(95., None);
        s.batteries.push(sample(10., None).batteries.remove(0));
        let d = e.step(&c, &s, caps(), 0);
        assert!(d.restore);
        assert!(!d.discharge);
    }
    #[test]
    fn stale_nan_empty_and_suspend_restore() {
        for mut s in [sample(f64::NAN, None), sample(50., None), sample(60., None)] {
            if s.lowest() == Some(50.) {
                s.fresh = false;
            }
            if s.lowest() == Some(60.) {
                s.batteries.clear();
            }
            assert!(
                Engine::default()
                    .step(&Config::default(), &s, caps(), 0)
                    .restore
            );
        }
        let mut e = Engine::default();
        e.step(&Config::default(), &sample(80., None), caps(), 0);
        assert!(
            e.step(&Config::default(), &sample(80., None), caps(), 100)
                .restore
        );
    }
    #[test]
    fn thermal_hysteresis_and_missing_sensor() {
        let mut e = Engine::default();
        let c = Config {
            heat_c: Some(42),
            ..Config::default()
        };
        assert!(e.step(&c, &sample(60., Some(42.)), caps(), 0).inhibit);
        assert!(e.step(&c, &sample(60., Some(40.)), caps(), 1).inhibit);
        assert!(!e.step(&c, &sample(60., Some(39.)), caps(), 2).inhibit);
        assert!(e.step(&c, &sample(60., None), caps(), 3).restore);
    }
    #[test]
    fn top_up_returns_to_saved_limit() {
        let mut e = Engine::default();
        let c = Config {
            mode: Mode::TopUp,
            ..Config::default()
        };
        assert_eq!(e.step(&c, &sample(80., None), caps(), 0).target, 100);
        let d = e.step(&c, &sample(100., None), caps(), 1);
        assert_eq!(d.target, 80);
        assert_eq!(d.phase, Phase::Complete);
        assert_eq!(e.step(&c, &sample(85., None), caps(), 2).target, 80);
    }
    #[test]
    fn calibration_reaches_recharge_without_deep_discharge() {
        let mut e = Engine::default();
        let c = Config {
            mode: Mode::Calibration,
            ..Config::default()
        };
        e.step(&c, &sample(100., None), caps(), 0);
        for now in (10..=600).step_by(10) {
            e.step(&c, &sample(100., None), caps(), now);
        }
        assert_eq!(
            e.step(&c, &sample(40., None), caps(), 610).phase,
            Phase::Discharging
        );
        let d = e.step(&c, &sample(25., None), caps(), 620);
        assert_eq!(d.phase, Phase::Recharging);
        assert!(!d.discharge);
        assert_eq!(
            e.step(&c, &sample(100., None), caps(), 630).phase,
            Phase::Complete
        );
    }
    #[test]
    fn unsupported_mode_and_invalid_config_restore() {
        let mut c = Config {
            mode: Mode::Calibration,
            ..Config::default()
        };
        assert!(
            Engine::default()
                .step(&c, &sample(80., None), Capabilities::default(), 0)
                .restore
        );
        c.limit = 0;
        assert!(
            Engine::default()
                .step(&c, &sample(80., None), caps(), 0)
                .restore
        );
    }
    #[test]
    fn interrupted_full_hold_cancels_instead_of_stalling() {
        for interruption in ["ac", "stale", "gap"] {
            let mut e = Engine::default();
            let c = Config {
                mode: Mode::Calibration,
                ..Config::default()
            };
            assert_eq!(
                e.step(&c, &sample(100., None), caps(), 0).phase,
                Phase::FullHold
            );
            let mut s = sample(100., None);
            if interruption == "ac" {
                s.ac = Some(false);
            }
            if interruption == "stale" {
                s.fresh = false;
            }
            let now = if interruption == "gap" { 60 } else { 10 };
            let d = e.step(&c, &s, caps(), now);
            assert!(d.restore);
            assert_eq!(d.phase, Phase::Complete);
            assert_eq!(
                e.step(&c, &sample(90., None), caps(), now + 10).phase,
                Phase::Complete
            );
        }
    }
}
