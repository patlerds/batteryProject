use crate::{dell, Capabilities, Snapshot};
use serde::{Deserialize, Serialize};

/// At or below this level (lowest pack) normal charging is always restored.
pub const SAFETY_FLOOR: f64 = 20.0;
/// The safety floor is released only once the lowest pack is back at this level.
pub const FLOOR_RELEASE: f64 = 25.0;
/// Packs commonly settle at 99% (or firmware reports Full) instead of exactly 100.
pub const FULL_PERCENT: f64 = 99.0;
/// A charge-to-full phase (top-up, calibration recharge) gives up after this long.
pub const CHARGE_PHASE_TIMEOUT_SECS: u64 = 4 * 60 * 60;
/// Any temporary mode (top-up, calibration) ends after this long, whatever its phase.
pub const TEMPORARY_MODE_TIMEOUT_SECS: u64 = 12 * 60 * 60;
/// Calibration holds at full for this long before discharging.
pub const FULL_HOLD_SECS: u64 = 600;
/// Hard stop for charging, independent of the user's heat setting and of the
/// safety floor. Li-ion cells are normally specified for charging up to about
/// 45 °C; charging hotter accelerates degradation and raises the risk of
/// lithium plating and thermal runaway. 50 °C leaves margin above that spec
/// while still catching a pack that is genuinely too hot to charge, so at or
/// above it charging stops even when the lowest pack is below the safety floor.
pub const CRITICAL_TEMP_C: f64 = 50.0;
/// The critical-heat stop is released only once the hottest pack reads below
/// this temperature (the top of the usual Li-ion charging range).
pub const CRITICAL_TEMP_RELEASE_C: f64 = 45.0;

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
        if self.heat_c.is_some_and(|t| f64::from(t) > CRITICAL_TEMP_C) {
            return Err(format!(
                "Heat threshold cannot be above the {CRITICAL_TEMP_C} °C critical stop"
            ));
        }
        if self.heat_c.is_some_and(|t| !(35..=50).contains(&t)) {
            return Err("Heat threshold must be 35–50 °C".into());
        }
        Ok(())
    }
    /// `validate` plus the limits of the enforcement mechanism. When hardware
    /// thresholds (`set_limit`) enforce the limit, (limit - band, limit) must be
    /// expressible within the documented Dell ranges (start 50–95, stop 55–100,
    /// gap >= 5). Top-up (100 - band, 100) is covered by the same gap rule.
    pub fn validate_for(&self, caps: Capabilities) -> Result<(), String> {
        self.validate()?;
        if caps.set_limit {
            for target in [self.limit, 100] {
                dell::thresholds_for(target, self.band, dell::DOCUMENTED_BOUNDS).map_err(|_| {
                    "Hardware limits need start 50–95%, stop 55–100% and a sailing band of at least 5%"
                        .to_string()
                })?;
            }
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

/// One-shot messages for the user. Each is emitted on a single `Decision`
/// (edge-triggered), whatever the provider's capabilities: on a monitor-only
/// device telling the user is the only protection there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notice {
    /// A top-up began charging to full.
    TopUpStarted,
    /// The top-up reached full; the battery now stays full while on AC.
    TopUpComplete,
    /// The critical-heat stop engaged: charging stopped, even below the floor.
    CriticalHeat,
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
    pub notice: Option<Notice>,
}

/// Hottest pack temperature from a fresh snapshot, if any pack reports one.
/// Any finite reading counts, even one outside the plausible sensor range.
fn hottest_reading(s: &Snapshot) -> Option<f64> {
    if !s.fresh {
        return None;
    }
    s.batteries
        .iter()
        .filter_map(|b| b.temperature_c)
        .filter(|t| t.is_finite())
        .reduce(f64::max)
}

pub struct Engine {
    phase: Phase,
    mode: Mode,
    holding: bool,
    hot: bool,
    hold_since: Option<u64>,
    last_time: Option<u64>,
    reminded: bool,
    /// Safety floor latch (enter at SAFETY_FLOOR, release at FLOOR_RELEASE).
    floor_active: bool,
    /// Start of the current charge-to-full phase (TopUp / Recharging).
    phase_since: Option<u64>,
    /// Start of the current temporary mode.
    mode_since: Option<u64>,
    /// A temporary mode just finished full: never actively drain it back to the
    /// limit while still on AC. Cleared by unplugging or dropping to the limit.
    keep_full: bool,
    last_limit: Option<u8>,
    /// Critical-heat latch (enter at CRITICAL_TEMP_C, release below
    /// CRITICAL_TEMP_RELEASE_C). Held while no fresh temperature is available.
    critical: bool,
    /// A top-up charge-to-full phase was entered during this step.
    top_up_began: bool,
    /// A top-up reached full during this step.
    top_up_finished: bool,
    /// Notices not yet delivered (at most one is delivered per step).
    notices: Vec<Notice>,
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
            floor_active: false,
            phase_since: None,
            mode_since: None,
            keep_full: false,
            last_limit: None,
            critical: false,
            top_up_began: false,
            top_up_finished: false,
            notices: Vec::new(),
        }
    }
}
impl Engine {
    fn cancel_temporary_mode(&mut self) {
        if self.mode != Mode::Limit {
            self.phase = Phase::Complete;
        }
        self.hold_since = None;
        self.phase_since = None;
        self.mode_since = None;
    }
    fn complete(&mut self) {
        self.phase = Phase::Complete;
        self.hold_since = None;
        self.phase_since = None;
        self.mode_since = None;
        self.keep_full = true;
    }
    /// Never charge and never discharge: the critical-heat stop. Overrides the
    /// safety floor, every mode and the user's heat setting.
    fn critical_stop(&self, mut d: Decision, caps: Capabilities) -> Decision {
        d.phase = self.phase;
        d.inhibit = caps.inhibit;
        d.discharge = false;
        d.restore = false;
        d.reminder = false;
        d.reason = "critical_heat";
        d
    }
    /// `now` is monotonic seconds, never wall-clock or a time zone dependent value.
    pub fn step(&mut self, c: &Config, s: &Snapshot, caps: Capabilities, now: u64) -> Decision {
        let was_critical = self.critical;
        self.top_up_began = false;
        self.top_up_finished = false;
        let mut d = self.decide(c, s, caps, now);
        // Edge-triggered notices, independent of capabilities. Critical heat
        // jumps the queue; anything else waits for the following steps.
        if self.critical && !was_critical {
            self.notices.insert(0, Notice::CriticalHeat);
        }
        let top_up_active = self.mode == Mode::TopUp && self.phase == Phase::TopUp;
        if self.top_up_began && (top_up_active || self.top_up_finished) {
            self.notices.push(Notice::TopUpStarted);
        }
        if self.top_up_finished {
            self.notices.push(Notice::TopUpComplete);
        }
        if !self.notices.is_empty() {
            d.notice = Some(self.notices.remove(0));
        }
        d
    }
    fn decide(&mut self, c: &Config, s: &Snapshot, caps: Capabilities, now: u64) -> Decision {
        let mut d = Decision {
            phase: self.phase,
            inhibit: false,
            discharge: false,
            target: c.limit,
            restore: false,
            reminder: false,
            reason: "Monitoring",
            notice: None,
        };
        let backwards = self
            .last_time
            .is_some_and(|last| now < last || now - last > 30);
        self.last_time = Some(now);
        // Critical-heat latch, whatever the configuration or capabilities. With
        // no fresh temperature it neither engages nor releases.
        if let Some(hottest) = hottest_reading(s) {
            if hottest >= CRITICAL_TEMP_C {
                self.critical = true;
            } else if hottest < CRITICAL_TEMP_RELEASE_C {
                self.critical = false;
            }
        }
        if c.validate_for(caps).is_err() || !s.valid() || backwards {
            self.holding = false;
            self.hot = false;
            self.hold_since = None;
            self.cancel_temporary_mode();
            d.phase = self.phase;
            if self.critical {
                return self.critical_stop(d, caps);
            }
            d.restore = true;
            d.reason = "Telemetry unavailable or resumed; restore safe defaults";
            return d;
        }
        let low = s.lowest().unwrap_or(0.0);
        let high = s.highest().unwrap_or(0.0);
        // Firmware "full" counts, so a pack that settles at 99% still completes.
        let full = s
            .batteries
            .iter()
            .all(|b| b.percent >= FULL_PERCENT || b.state.eq_ignore_ascii_case("full"));
        // Re-arm the reminder (and drop a stale hold) on unplug or limit change.
        if s.ac != Some(true) {
            self.reminded = false;
            self.keep_full = false;
        }
        if self.last_limit != Some(c.limit) {
            self.last_limit = Some(c.limit);
            self.reminded = false;
            self.holding = false;
        }
        // Re-arm once back under the sailing band, before any early return
        // (floor, unsupported mode, heat) can skip the check further down.
        if high <= f64::from(c.limit.saturating_sub(c.band)) {
            self.reminded = false;
        }
        if self.mode != c.mode {
            self.mode = c.mode;
            self.hold_since = None;
            self.holding = false;
            self.phase = match c.mode {
                Mode::Limit => Phase::Limit,
                Mode::TopUp => Phase::TopUp,
                Mode::Calibration => Phase::TopUp,
            };
            let temporary = c.mode != Mode::Limit;
            self.phase_since = temporary.then_some(now);
            self.mode_since = temporary.then_some(now);
            self.top_up_began = c.mode == Mode::TopUp;
        }
        // Critical heat beats everything, including the safety floor.
        if self.critical {
            return self.critical_stop(d, caps);
        }
        // Floor precedence: the floor wins over limit holds, normal heat
        // protection and modes.
        if low <= SAFETY_FLOOR {
            self.floor_active = true;
        } else if low >= FLOOR_RELEASE {
            self.floor_active = false;
        }
        if self.floor_active {
            self.holding = false;
            self.hot = false;
            if self.phase == Phase::Discharging {
                self.phase = Phase::Recharging;
                self.phase_since = Some(now);
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
            // Nothing here can limit charging, so the pack charges to full on
            // its own; still follow the top-up so the user is told when it is.
            if self.phase == Phase::TopUp && full {
                self.complete();
                self.top_up_finished = true;
            }
            d.phase = self.phase;
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
        let elapsed = |since: Option<u64>| now.saturating_sub(since.unwrap_or(now));
        let mut timed_out = false;
        match self.phase {
            Phase::TopUp if full => {
                if c.mode == Mode::Calibration {
                    self.phase = Phase::FullHold;
                    self.hold_since = Some(now);
                    self.phase_since = None;
                } else {
                    self.complete();
                    self.top_up_finished = c.mode == Mode::TopUp;
                }
            }
            Phase::Recharging if full => self.complete(),
            Phase::TopUp | Phase::Recharging
                if elapsed(self.phase_since) >= CHARGE_PHASE_TIMEOUT_SECS =>
            {
                timed_out = true;
                self.complete();
            }
            Phase::FullHold if elapsed(self.hold_since) >= FULL_HOLD_SECS => {
                self.phase = Phase::Discharging;
                self.hold_since = None;
            }
            Phase::Discharging if low <= FLOOR_RELEASE => {
                self.phase = Phase::Recharging;
                self.phase_since = Some(now);
            }
            _ => {}
        }
        if !matches!(self.phase, Phase::Limit | Phase::Complete)
            && elapsed(self.mode_since) >= TEMPORARY_MODE_TIMEOUT_SECS
        {
            timed_out = true;
            self.complete();
        }
        let to_full = matches!(self.phase, Phase::TopUp | Phase::Recharging);
        d.phase = self.phase;
        d.target = if to_full { 100 } else { c.limit };
        let band_floor = f64::from(d.target.saturating_sub(c.band));
        if high >= f64::from(d.target) {
            self.holding = true;
        }
        if high <= band_floor {
            self.holding = false;
            self.reminded = false;
        }
        if high <= f64::from(c.limit) {
            self.keep_full = false;
        }
        // Upper-limit holds use the highest pack, but never while another pack is
        // still below the sailing band (multi-pack: Dell charges packs in turn, so
        // inhibit would pin the low pack). Charge-to-full phases never hold.
        let limit_hold = self.holding && !to_full && low > band_floor;
        d.inhibit = caps.inhibit
            && (limit_hold || matches!(self.phase, Phase::FullHold | Phase::Discharging));
        // Discharge-to-limit only in limit phases, never right after a top-up or
        // calibration finished full on AC, and never while any pack is below band.
        let drain_to_limit = c.discharge
            && matches!(self.phase, Phase::Limit | Phase::Complete)
            && !self.keep_full
            && high > f64::from(c.limit)
            && low > band_floor;
        d.discharge = caps.force_discharge && (self.phase == Phase::Discharging || drain_to_limit);
        d.reason = if timed_out {
            "Temporary mode timed out; returned to the saved limit"
        } else if self.holding && !to_full && !limit_hold && caps.inhibit {
            "A battery is below the sailing band; charging allowed"
        } else if caps.set_limit || caps.inhibit {
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
    fn packs(percents: &[f64]) -> Snapshot {
        let mut s = sample(percents[0], None);
        for p in &percents[1..] {
            s.batteries.push(sample(*p, None).batteries.remove(0));
        }
        s
    }
    #[test]
    fn calibration_full_hold_never_force_discharges() {
        let mut e = Engine::default();
        let c = Config {
            mode: Mode::Calibration,
            discharge: true,
            ..Config::default()
        };
        let mut now = 0;
        let d = e.step(&c, &sample(100., None), caps(), now);
        assert_eq!(d.phase, Phase::FullHold);
        while now < FULL_HOLD_SECS - 10 {
            now += 10;
            let d = e.step(&c, &sample(100., None), caps(), now);
            assert_eq!(d.phase, Phase::FullHold);
            assert!(d.inhibit && !d.discharge, "t={now}");
        }
        // TopUp and Recharging never discharge either.
        let mut e = Engine::default();
        let d = e.step(&c, &sample(90., None), caps(), 0);
        assert_eq!(d.phase, Phase::TopUp);
        assert!(!d.discharge && !d.inhibit);
    }
    #[test]
    fn top_up_completes_at_99_or_firmware_full() {
        let c = Config {
            mode: Mode::TopUp,
            ..Config::default()
        };
        let mut e = Engine::default();
        e.step(&c, &sample(90., None), caps(), 0);
        assert_eq!(
            e.step(&c, &sample(99., None), caps(), 10).phase,
            Phase::Complete
        );
        let mut e = Engine::default();
        let mut s = sample(97., None);
        e.step(&c, &s, caps(), 0);
        s.batteries[0].state = "Full".into();
        assert_eq!(e.step(&c, &s, caps(), 10).phase, Phase::Complete);
        // Calibration recharge completes at 99 too.
        let cal = Config {
            mode: Mode::Calibration,
            ..Config::default()
        };
        let mut e = Engine::default();
        for now in (0..=FULL_HOLD_SECS).step_by(10) {
            e.step(&cal, &sample(99., None), caps(), now);
        }
        let t = FULL_HOLD_SECS + 10;
        assert_eq!(
            e.step(&cal, &sample(25., None), caps(), t).phase,
            Phase::Recharging
        );
        assert_eq!(
            e.step(&cal, &sample(99., None), caps(), t + 10).phase,
            Phase::Complete
        );
    }
    #[test]
    fn temporary_modes_time_out() {
        let c = Config {
            mode: Mode::TopUp,
            ..Config::default()
        };
        let mut e = Engine::default();
        let mut now = 0;
        while now < CHARGE_PHASE_TIMEOUT_SECS {
            assert_eq!(
                e.step(&c, &sample(95., None), caps(), now).phase,
                Phase::TopUp
            );
            now += 10;
        }
        let d = e.step(&c, &sample(95., None), caps(), now);
        assert_eq!(d.phase, Phase::Complete);
        assert_eq!(d.target, 80);
        // A calibration stuck discharging ends at the overall mode bound.
        let cal = Config {
            mode: Mode::Calibration,
            ..Config::default()
        };
        let mut e = Engine::default();
        let mut now = 0;
        let mut last = Phase::TopUp;
        while now <= TEMPORARY_MODE_TIMEOUT_SECS {
            let p = if now <= FULL_HOLD_SECS { 100. } else { 60. };
            last = e.step(&cal, &sample(p, None), caps(), now).phase;
            if now == TEMPORARY_MODE_TIMEOUT_SECS - 10 {
                assert_eq!(last, Phase::Discharging);
            }
            now += 10;
        }
        assert_eq!(last, Phase::Complete);
    }
    #[test]
    fn safety_floor_has_hysteresis() {
        let mut e = Engine::default();
        let c = Config::default();
        for (i, (p, restore)) in [
            (22., false),
            (20., true),
            (21., true),
            (24.5, true),
            (25., false),
            (23., false),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                e.step(&c, &sample(p, None), caps(), i as u64).restore,
                restore,
                "{p}"
            );
        }
    }
    #[test]
    fn mixed_packs_floor_beats_limit_hold_and_top_up_completes() {
        let c = Config::default();
        let mut e = Engine::default();
        for (i, low) in [21., 20., 20.5, 22., 30.].into_iter().enumerate() {
            let d = e.step(&c, &packs(&[80., low]), caps(), i as u64);
            assert!(!d.inhibit, "80/{low}");
            assert!(!d.discharge, "80/{low}");
        }
        // Both packs within band: the highest pack drives the hold.
        assert!(e.step(&c, &packs(&[80., 77.]), caps(), 10).inhibit);
        let t = Config {
            mode: Mode::TopUp,
            ..Config::default()
        };
        let mut e = Engine::default();
        let d = e.step(&t, &packs(&[100., 90.]), caps(), 0);
        assert_eq!(d.phase, Phase::TopUp);
        assert!(!d.inhibit);
        let d = e.step(&t, &packs(&[100., 97.]), caps(), 10);
        assert!(!d.inhibit);
        assert_eq!(
            e.step(&t, &packs(&[100., 99.]), caps(), 20).phase,
            Phase::Complete
        );
    }
    #[test]
    fn top_up_result_is_not_drained_on_ac() {
        let mut e = Engine::default();
        let c = Config {
            mode: Mode::TopUp,
            discharge: true,
            ..Config::default()
        };
        e.step(&c, &sample(90., None), caps(), 0);
        let d = e.step(&c, &sample(100., None), caps(), 10);
        assert_eq!(d.phase, Phase::Complete);
        assert!(!d.discharge);
        let limit = Config {
            discharge: true,
            ..Config::default()
        };
        for now in (20..200).step_by(10) {
            let d = e.step(&limit, &sample(100., None), caps(), now);
            assert!(!d.discharge && d.inhibit, "t={now}");
        }
        // After an unplug, the explicit discharge option applies again.
        let mut s = sample(95., None);
        s.ac = Some(false);
        e.step(&limit, &s, caps(), 200);
        assert!(e.step(&limit, &sample(95., None), caps(), 210).discharge);
    }
    #[test]
    fn reminder_rearms_on_unplug_and_limit_change() {
        let none = Capabilities::default();
        let c = Config::default();
        let mut e = Engine::default();
        assert!(e.step(&c, &sample(80., None), none, 0).reminder);
        let mut s = sample(77., None);
        s.ac = Some(false);
        e.step(&c, &s, none, 10);
        assert!(!e.step(&c, &sample(77., None), none, 20).reminder);
        assert!(e.step(&c, &sample(80., None), none, 30).reminder);
        // Limit raised then reached again: new reminder.
        let higher = Config {
            limit: 85,
            ..Config::default()
        };
        assert!(!e.step(&higher, &sample(82., None), none, 40).reminder);
        assert!(e.step(&higher, &sample(85., None), none, 50).reminder);
        // A stale hold does not survive a raised limit.
        let mut e = Engine::default();
        assert!(e.step(&c, &sample(82., None), caps(), 0).inhibit);
        let raised = Config {
            limit: 85,
            ..Config::default()
        };
        assert!(!e.step(&raised, &sample(82., None), caps(), 10).inhibit);
    }
    #[test]
    fn reminder_rearms_below_band_even_during_early_returns() {
        let none = Capabilities::default();
        let c = Config::default();
        // An unsupported mode or heat setting returns early on AC; dropping
        // below the band meanwhile must still re-arm the reminder.
        let top_up = Config {
            mode: Mode::TopUp,
            ..Config::default()
        };
        let heat = Config {
            heat_c: Some(45),
            ..Config::default()
        };
        for detour in [top_up, heat] {
            let mut e = Engine::default();
            assert!(e.step(&c, &sample(80., None), none, 0).reminder);
            assert!(e.step(&detour, &sample(70., None), none, 10).restore);
            assert!(!e.step(&c, &sample(78., None), none, 20).reminder);
            assert!(e.step(&c, &sample(80., None), none, 30).reminder);
        }
        // Multi-pack: the safety floor (low pack) returns early while the high
        // pack falls below the band; the reminder re-arms all the same.
        let mut e = Engine::default();
        assert!(e.step(&c, &packs(&[80., 30.]), none, 0).reminder);
        assert!(e.step(&c, &packs(&[70., 20.]), none, 10).restore);
        assert!(!e.step(&c, &packs(&[78., 30.]), none, 20).reminder);
        assert!(e.step(&c, &packs(&[80., 30.]), none, 30).reminder);
        // Still no spam while above the band.
        assert!(!e.step(&c, &packs(&[79., 30.]), none, 40).reminder);
    }
    #[test]
    fn hardware_limits_must_be_dell_expressible() {
        let hw = Capabilities {
            set_limit: true,
            ..Capabilities::default()
        };
        for (limit, band) in [(80, 2), (60, 20), (52, 2), (54, 4)] {
            let c = Config {
                limit,
                band,
                ..Config::default()
            };
            assert!(c.validate().is_ok());
            assert!(c.validate_for(Capabilities::default()).is_ok());
            assert!(c.validate_for(hw).is_err(), "{limit}/{band}");
            assert!(
                Engine::default()
                    .step(&c, &sample(70., None), hw, 0)
                    .restore
            );
        }
        for (limit, band) in [(80, 5), (55, 5), (100, 5), (70, 20)] {
            let c = Config {
                limit,
                band,
                ..Config::default()
            };
            assert!(c.validate_for(hw).is_ok(), "{limit}/{band}");
        }
    }
    #[test]
    fn critical_heat_beats_the_safety_floor() {
        for heat_c in [None, Some(42)] {
            let c = Config {
                heat_c,
                ..Config::default()
            };
            let mut e = Engine::default();
            let d = e.step(&c, &sample(10., Some(CRITICAL_TEMP_C)), caps(), 0);
            assert!(d.inhibit && !d.restore && !d.discharge, "{heat_c:?}");
            assert_eq!(d.reason, "critical_heat");
            // Multi-pack: one hot pack stops charging even with a pack at 15%.
            let mut s = packs(&[15., 60.]);
            s.batteries[1].temperature_c = Some(55.);
            let d = Engine::default().step(&c, &s, caps(), 0);
            assert!(d.inhibit && !d.restore, "{heat_c:?}");
        }
        // Every mode, and even an invalid or stale reading once latched.
        for mode in [Mode::Limit, Mode::TopUp, Mode::Calibration] {
            let c = Config {
                mode,
                ..Config::default()
            };
            let d = Engine::default().step(&c, &sample(30., Some(51.)), caps(), 0);
            assert!(d.inhibit && !d.discharge, "{mode:?}");
            assert_eq!(d.reason, "critical_heat");
        }
        let mut e = Engine::default();
        e.step(&Config::default(), &sample(30., Some(51.)), caps(), 0);
        let mut stale = sample(10., Some(51.));
        stale.fresh = false;
        let d = e.step(&Config::default(), &stale, caps(), 10);
        assert!(d.inhibit && !d.restore);
        // No temperature at all: the critical tier never triggers.
        let d = Engine::default().step(&Config::default(), &sample(10., None), caps(), 0);
        assert!(d.restore && !d.inhibit);
    }
    #[test]
    fn normal_heat_below_critical_does_not_beat_the_floor() {
        let c = Config {
            heat_c: Some(40),
            ..Config::default()
        };
        let mut e = Engine::default();
        let d = e.step(&c, &sample(10., Some(49.9)), caps(), 0);
        assert!(d.restore && !d.inhibit);
        assert_ne!(d.reason, "critical_heat");
        // Above the floor, normal heat protection still pauses charging.
        assert!(e.step(&c, &sample(30., Some(45.)), caps(), 10).inhibit);
    }
    #[test]
    fn critical_heat_latch_has_hysteresis() {
        let c = Config::default();
        let mut e = Engine::default();
        for (i, (t, critical)) in [
            (49.9, false),
            (50., true),
            (47., true),
            (45., true),
            (44.9, false),
            (49., false),
            (50.5, true),
        ]
        .into_iter()
        .enumerate()
        {
            let d = e.step(&c, &sample(15., Some(t)), caps(), i as u64);
            assert_eq!(d.reason == "critical_heat", critical, "{t}");
            assert_eq!(d.inhibit, critical, "{t}");
            assert_eq!(d.restore, !critical, "{t}");
        }
    }
    #[test]
    fn critical_heat_never_discharges() {
        let c = Config {
            discharge: true,
            ..Config::default()
        };
        let mut e = Engine::default();
        assert!(e.step(&c, &sample(95., Some(30.)), caps(), 0).discharge);
        let d = e.step(&c, &sample(95., Some(55.)), caps(), 10);
        assert!(!d.discharge && d.inhibit);
        // Calibration mid-discharge: the discharge stops too.
        let cal = Config {
            mode: Mode::Calibration,
            ..Config::default()
        };
        let mut e = Engine::default();
        let mut now = 0;
        while now <= FULL_HOLD_SECS {
            e.step(&cal, &sample(100., Some(30.)), caps(), now);
            now += 10;
        }
        let d = e.step(&cal, &sample(90., Some(30.)), caps(), now);
        assert_eq!(d.phase, Phase::Discharging);
        assert!(d.discharge);
        let d = e.step(&cal, &sample(89., Some(52.)), caps(), now + 10);
        assert!(!d.discharge && d.inhibit);
    }
    fn notices(
        e: &mut Engine,
        c: &Config,
        steps: &[(f64, Option<f64>)],
        caps: Capabilities,
    ) -> Vec<Notice> {
        steps
            .iter()
            .enumerate()
            .filter_map(|(i, (p, t))| e.step(c, &sample(*p, *t), caps, i as u64 * 10).notice)
            .collect()
    }
    #[test]
    fn notices_fire_exactly_once_with_and_without_capabilities() {
        let top_up = Config {
            mode: Mode::TopUp,
            ..Config::default()
        };
        let run = [
            (80., None),
            (90., None),
            (99., None),
            (100., None),
            (100., None),
        ];
        for c in [caps(), Capabilities::default()] {
            let mut e = Engine::default();
            let d = e.step(&top_up, &sample(80., None), c, 0);
            assert_eq!(d.notice, Some(Notice::TopUpStarted), "{c:?}");
            assert_eq!(e.step(&top_up, &sample(90., None), c, 10).notice, None);
            let d = e.step(&top_up, &sample(99., None), c, 20);
            assert_eq!(d.notice, Some(Notice::TopUpComplete), "{c:?}");
            assert_eq!(d.phase, Phase::Complete);
            let mut e = Engine::default();
            assert_eq!(
                notices(&mut e, &top_up, &run, c),
                [Notice::TopUpStarted, Notice::TopUpComplete],
                "{c:?}"
            );
            // Already full when switched on: started, then complete next step.
            let mut e = Engine::default();
            assert_eq!(
                notices(
                    &mut e,
                    &top_up,
                    &[(100., None), (100., None), (100., None)],
                    c
                ),
                [Notice::TopUpStarted, Notice::TopUpComplete],
                "{c:?}"
            );
            // Critical heat: once per engagement, again only after release.
            let mut e = Engine::default();
            let heat = [
                (60., Some(30.)),
                (60., Some(50.)),
                (60., Some(55.)),
                (60., Some(46.)),
                (60., Some(44.)),
                (60., Some(40.)),
                (60., Some(51.)),
                (60., Some(51.)),
            ];
            let mut seen = Vec::new();
            for (i, (p, t)) in heat.into_iter().enumerate() {
                let d = e.step(&Config::default(), &sample(p, t), c, i as u64 * 10);
                assert!(!d.discharge);
                seen.push(d.notice);
            }
            let critical = Some(Notice::CriticalHeat);
            assert_eq!(
                seen,
                [None, critical, None, None, None, None, critical, None],
                "{c:?}"
            );
        }
        // A top-up cancelled by an unplug in the same step never announces.
        let mut s = sample(80., None);
        s.ac = Some(false);
        assert_eq!(Engine::default().step(&top_up, &s, caps(), 0).notice, None);
        // Critical heat while starting a top-up: both are delivered, heat first.
        let mut e = Engine::default();
        assert_eq!(
            notices(
                &mut e,
                &top_up,
                &[(80., Some(55.)), (80., Some(55.)), (80., Some(55.))],
                caps()
            ),
            [Notice::CriticalHeat, Notice::TopUpStarted]
        );
    }
    #[test]
    fn top_up_then_stays_full_without_draining() {
        let c = Config {
            mode: Mode::TopUp,
            discharge: true,
            ..Config::default()
        };
        let mut e = Engine::default();
        e.step(&c, &sample(90., None), caps(), 0);
        assert_eq!(
            e.step(&c, &sample(100., None), caps(), 10).notice,
            Some(Notice::TopUpComplete)
        );
        for now in (20..400).step_by(10) {
            let d = e.step(&c, &sample(100., None), caps(), now);
            assert_eq!(d.phase, Phase::Complete);
            assert!(!d.discharge && !d.restore, "t={now}");
            assert_eq!(d.notice, None);
        }
    }
    #[test]
    fn heat_threshold_above_critical_is_rejected() {
        let hot = Config {
            heat_c: Some(51),
            ..Config::default()
        };
        let err = hot.validate().unwrap_err();
        assert!(err.contains("critical"), "{err}");
        assert!(hot.validate_for(caps()).is_err());
        assert!(Config {
            heat_c: Some(50),
            ..Config::default()
        }
        .validate()
        .is_ok());
    }
    #[test]
    fn notice_serializes_snake_case() {
        let d = Engine::default().step(
            &Config {
                mode: Mode::TopUp,
                ..Config::default()
            },
            &sample(80., None),
            caps(),
            0,
        );
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains(r#""notice":"top_up_started""#), "{json}");
        for (n, name) in [
            (Notice::TopUpStarted, "\"top_up_started\""),
            (Notice::TopUpComplete, "\"top_up_complete\""),
            (Notice::CriticalHeat, "\"critical_heat\""),
        ] {
            assert_eq!(serde_json::to_string(&n).unwrap(), name);
            assert_eq!(serde_json::from_str::<Notice>(name).unwrap(), n);
        }
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
