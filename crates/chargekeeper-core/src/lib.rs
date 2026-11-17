pub mod dell;
pub mod policy;
pub mod protocol;
pub mod simulator;
pub mod storage;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub set_limit: bool,
    pub thresholds: bool,
    pub inhibit: bool,
    pub force_discharge: bool,
    pub temperature: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Battery {
    pub percent: f64,
    pub state: String,
    pub power_w: Option<f64>,
    pub temperature_c: Option<f64>,
    pub cycles: Option<u32>,
    pub health: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub batteries: Vec<Battery>,
    pub ac: Option<bool>,
    pub fresh: bool,
}

impl Snapshot {
    /// The lowest pack drives safety decisions. No averaging can conceal a low pack.
    pub fn lowest(&self) -> Option<f64> {
        self.batteries.iter().map(|b| b.percent).reduce(f64::min)
    }
    pub fn highest(&self) -> Option<f64> {
        self.batteries.iter().map(|b| b.percent).reduce(f64::max)
    }
    pub fn valid(&self) -> bool {
        self.fresh
            && !self.batteries.is_empty()
            && self.batteries.iter().all(|b| {
                b.percent.is_finite()
                    && (0.0..=100.0).contains(&b.percent)
                    && b.temperature_c
                        .is_none_or(|t| t.is_finite() && (-30.0..=100.0).contains(&t))
            })
    }
}

pub trait BatteryProvider {
    fn snapshot(&mut self) -> Result<Snapshot, String>;
    fn capabilities(&self) -> Capabilities;
}

pub trait ChargeController {
    fn set_thresholds(&mut self, start: u8, stop: u8) -> Result<(), String>;
    fn set_charge_inhibit(&mut self, inhibit: bool) -> Result<(), String>;
    fn force_discharge(&mut self, enabled: bool) -> Result<(), String>;
    fn restore_defaults(&mut self) -> Result<(), String>;
}
