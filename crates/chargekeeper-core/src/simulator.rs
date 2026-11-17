use crate::policy::{Config, Decision, Engine, Mode, Notice, Phase};
use crate::{Battery, BatteryProvider, Capabilities, ChargeController, Snapshot};
use serde::Serialize;

#[derive(Serialize)]
pub struct Simulation {
    pub snapshot: Snapshot,
    pub phase: Phase,
    pub reason: String,
    pub notice: Option<Notice>,
    pub start: u8,
    pub stop: u8,
}

pub struct Simulator {
    pub percent: f64,
    pub temperature: f64,
    pub ac: bool,
    start: u8,
    stop: u8,
    inhibit: bool,
    discharge: bool,
    charging: bool,
    time: u64,
    engine: Engine,
}
impl Default for Simulator {
    fn default() -> Self {
        Self {
            percent: 68.,
            temperature: 31.,
            ac: true,
            start: 75,
            stop: 80,
            inhibit: false,
            discharge: false,
            charging: true,
            time: 0,
            engine: Engine::default(),
        }
    }
}
impl BatteryProvider for Simulator {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            set_limit: true,
            thresholds: true,
            inhibit: true,
            force_discharge: true,
            temperature: true,
        }
    }
    fn snapshot(&mut self) -> Result<Snapshot, String> {
        Ok(Snapshot {
            batteries: vec![Battery {
                percent: self.percent,
                state: if !self.ac || self.discharge {
                    "discharging"
                } else if self.charging && !self.inhibit {
                    "charging"
                } else {
                    "not charging"
                }
                .into(),
                power_w: Some(if !self.ac || self.discharge {
                    -12.4
                } else if self.charging && !self.inhibit {
                    24.6
                } else {
                    0.
                }),
                temperature_c: Some(self.temperature),
                cycles: Some(142),
                health: Some(96.2),
            }],
            ac: Some(self.ac),
            fresh: true,
        })
    }
}
impl ChargeController for Simulator {
    fn set_thresholds(&mut self, start: u8, stop: u8) -> Result<(), String> {
        // Honest: reject exactly what the documented Dell interface rejects.
        crate::dell::DOCUMENTED_BOUNDS.validate(start, stop)?;
        self.start = start;
        self.stop = stop;
        Ok(())
    }
    fn set_charge_inhibit(&mut self, inhibit: bool) -> Result<(), String> {
        self.inhibit = inhibit;
        Ok(())
    }
    fn force_discharge(&mut self, enabled: bool) -> Result<(), String> {
        self.discharge = enabled;
        Ok(())
    }
    fn restore_defaults(&mut self) -> Result<(), String> {
        self.start = 0;
        self.stop = 100;
        self.inhibit = false;
        self.discharge = false;
        self.charging = true;
        Ok(())
    }
}
impl Simulator {
    pub fn reset_policy(&mut self) {
        self.engine = Engine::default();
    }
    pub fn step(&mut self, config: &mut Config) -> Result<Simulation, String> {
        config.validate_for(self.capabilities())?;
        let snap = self.snapshot()?;
        let decision: Decision = self
            .engine
            .step(config, &snap, self.capabilities(), self.time);
        self.time += 10;
        if decision.restore {
            self.restore_defaults()?;
        } else {
            self.set_thresholds(decision.target.saturating_sub(config.band), decision.target)?;
            self.set_charge_inhibit(decision.inhibit)?;
            self.force_discharge(decision.discharge)?;
        }
        if self.percent >= f64::from(self.stop) {
            self.charging = false;
        }
        if self.percent <= f64::from(self.start) {
            self.charging = true;
        }
        if !self.ac || self.discharge {
            self.percent = (self.percent - 1.5).max(0.);
        } else if self.charging && !self.inhibit {
            self.percent = (self.percent + 1.5).min(100.);
        }
        if decision.phase == Phase::Complete {
            config.mode = Mode::Limit;
        }
        Ok(Simulation {
            snapshot: self.snapshot()?,
            phase: decision.phase,
            reason: decision.reason.into(),
            notice: decision.notice,
            start: self.start,
            stop: self.stop,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integrated_policy_holds_at_limit_and_sails() {
        let mut s = Simulator::default();
        let mut c = Config::default();
        for _ in 0..30 {
            s.step(&mut c).unwrap();
        }
        assert!((80.0..82.0).contains(&s.percent));
        s.percent = 74.;
        s.step(&mut c).unwrap();
        assert!(s.percent > 74.);
    }
    #[test]
    fn topup_completes_and_returns_to_limit() {
        let mut s = Simulator::default();
        let mut c = Config {
            mode: Mode::TopUp,
            ..Config::default()
        };
        for _ in 0..30 {
            s.step(&mut c).unwrap();
        }
        assert_eq!(c.mode, Mode::Limit);
        assert_eq!(s.stop, 80);
    }
    #[test]
    fn low_battery_restoration_resumes_charging_after_holding() {
        let mut s = Simulator::default();
        let mut c = Config::default();
        for _ in 0..30 {
            s.step(&mut c).unwrap();
        }
        assert!(!s.charging);
        s.percent = 10.;
        s.step(&mut c).unwrap();
        assert!(s.percent > 10.);
        assert!(s.charging);
    }
    #[test]
    fn rejects_settings_dell_hardware_cannot_express() {
        for (limit, band) in [(80, 2), (60, 20), (52, 2)] {
            let mut c = Config {
                limit,
                band,
                ..Config::default()
            };
            assert!(c.validate().is_ok());
            assert!(Simulator::default().step(&mut c).is_err(), "{limit}/{band}");
        }
        let mut s = Simulator::default();
        assert!(s.set_thresholds(78, 80).is_err());
        assert!(s.set_thresholds(40, 60).is_err());
        assert!(s.set_thresholds(75, 80).is_ok());
    }
    #[test]
    fn topup_with_discharge_option_stays_full_until_unplugged() {
        let mut s = Simulator::default();
        let mut c = Config {
            mode: Mode::TopUp,
            discharge: true,
            ..Config::default()
        };
        for _ in 0..40 {
            s.step(&mut c).unwrap();
        }
        assert_eq!(c.mode, Mode::Limit);
        assert!(s.percent >= 99., "{}", s.percent);
        let before = s.percent;
        for _ in 0..20 {
            s.step(&mut c).unwrap();
        }
        assert!(s.percent >= before, "drained on AC: {}", s.percent);
    }
}
