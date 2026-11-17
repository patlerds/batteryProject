//! Pure Dell command/schema adapter. Production execution is deliberately not
//! connected until authenticated service and real-device qualification exist.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub start_min: u8,
    pub start_max: u8,
    pub stop_min: u8,
    pub stop_max: u8,
    pub gap: u8,
}
impl Bounds {
    pub fn validate(&self, start: u8, stop: u8) -> Result<(), String> {
        if self.start_min > self.start_max
            || self.stop_min > self.stop_max
            || self.start_max > 100
            || self.stop_max > 100
            || self.gap == 0
            || self.gap > 100
        {
            return Err("Invalid device-reported bounds".into());
        }
        if !(self.start_min..=self.start_max).contains(&start)
            || !(self.stop_min..=self.stop_max).contains(&stop)
            || stop <= start
            || stop - start < self.gap
        {
            return Err("Thresholds outside device bounds or minimum gap".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BiosConfig {
    Standard,
    Express,
    PrimAcUse,
    Adaptive,
    Custom { start: u8, stop: u8 },
}

/// Accept only the documented single-attribute response. Extra banners are OK;
/// duplicate values, bare 'Custom', malformed and ambiguous output are not.
pub fn parse_cctk(output: &str) -> Result<BiosConfig, String> {
    let mut lines = output.lines().filter_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("PrimaryBattChargeCfg")
            .then_some(value.trim())
    });
    let value = lines.next().ok_or("Charge configuration not found")?;
    if lines.next().is_some() {
        return Err("Ambiguous charge configuration".into());
    }
    match value.to_ascii_lowercase().as_str() {
        "standard" => Ok(BiosConfig::Standard),
        "express" => Ok(BiosConfig::Express),
        "primacuse" => Ok(BiosConfig::PrimAcUse),
        "adaptive" => Ok(BiosConfig::Adaptive),
        custom => {
            let range = custom.strip_prefix("custom:").ok_or("Unknown BIOS mode")?;
            let (start, stop) = range.split_once('-').ok_or("Missing Custom thresholds")?;
            let start = start.parse::<u8>().map_err(|_| "Invalid start threshold")?;
            let stop = stop.parse::<u8>().map_err(|_| "Invalid stop threshold")?;
            if start >= stop || stop > 100 {
                return Err("Invalid Custom thresholds".into());
            }
            Ok(BiosConfig::Custom { start, stop })
        }
    }
}

pub fn cctk_args(config: &BiosConfig, bounds: Option<Bounds>) -> Result<Vec<String>, String> {
    let value = match config {
        BiosConfig::Standard => "Standard".into(),
        BiosConfig::Express => "Express".into(),
        BiosConfig::PrimAcUse => "PrimAcUse".into(),
        BiosConfig::Adaptive => "Adaptive".into(),
        BiosConfig::Custom { start, stop } => {
            bounds
                .ok_or("Device bounds have not been discovered")?
                .validate(*start, *stop)?;
            format!("Custom:{start}-{stop}")
        }
    };
    Ok(vec![format!("--PrimaryBattChargeCfg={value}")])
}

pub trait DellTransport {
    fn read(&mut self) -> Result<BiosConfig, String>;
    fn write(&mut self, config: &BiosConfig) -> Result<(), String>;
}
pub trait Journal {
    /// Implementation must durably persist original state BEFORE returning.
    fn record_original(&mut self, original: &BiosConfig) -> Result<(), String>;
}
pub fn apply_verified(
    transport: &mut impl DellTransport,
    journal: &mut impl Journal,
    config: &BiosConfig,
    bounds: Option<Bounds>,
) -> Result<BiosConfig, String> {
    cctk_args(config, bounds)?;
    let original = transport.read()?;
    journal.record_original(&original)?;
    transport.write(config)?;
    let actual = transport.read()?;
    if actual != *config {
        return Err("BIOS readback mismatch; enforcement is unconfirmed".into());
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    const B: Bounds = Bounds {
        start_min: 50,
        start_max: 95,
        stop_min: 55,
        stop_max: 100,
        gap: 5,
    };
    #[test]
    fn parse_documented_output() {
        assert_eq!(
            parse_cctk("Header\r\nPrimaryBattChargeCfg=Custom:75-80\r\n").unwrap(),
            BiosConfig::Custom {
                start: 75,
                stop: 80
            }
        );
        assert_eq!(
            parse_cctk("PrimaryBattChargeCfg=Standard").unwrap(),
            BiosConfig::Standard
        );
    }
    #[test]
    fn reject_ambiguous_and_hostile_output() {
        for value in [
            "",
            "PrimaryBattChargeCfg=Custom",
            "PrimaryBattChargeCfg=Custom:80-75",
            "PrimaryBattChargeCfg=Custom:75-101",
            "PrimaryBattChargeCfg=Custom:75-80 & calc",
            "PrimaryBattChargeCfg=Standard\nPrimaryBattChargeCfg=Adaptive",
        ] {
            assert!(parse_cctk(value).is_err(), "{value}");
        }
    }
    #[test]
    fn no_threshold_write_without_device_bounds() {
        let cfg = BiosConfig::Custom {
            start: 75,
            stop: 80,
        };
        assert!(cctk_args(&cfg, None).is_err());
        assert_eq!(
            cctk_args(&cfg, Some(B)).unwrap(),
            ["--PrimaryBattChargeCfg=Custom:75-80"]
        );
        for (start, stop) in [(50, 50), (75, 79), (49, 80), (96, 100)] {
            assert!(B.validate(start, stop).is_err());
        }
        assert!(B.validate(50, 55).is_ok());
    }
    struct Mock {
        current: BiosConfig,
        ignore: bool,
        writes: u8,
    }
    impl DellTransport for Mock {
        fn read(&mut self) -> Result<BiosConfig, String> {
            Ok(self.current.clone())
        }
        fn write(&mut self, c: &BiosConfig) -> Result<(), String> {
            self.writes += 1;
            if !self.ignore {
                self.current = c.clone();
            }
            Ok(())
        }
    }
    struct Log {
        fail: bool,
        original: Option<BiosConfig>,
    }
    impl Journal for Log {
        fn record_original(&mut self, c: &BiosConfig) -> Result<(), String> {
            if self.fail {
                return Err("Disk failure".into());
            }
            self.original.get_or_insert(c.clone());
            Ok(())
        }
    }
    #[test]
    fn journal_failure_prevents_write_and_readback_failure_is_error() {
        let mut m = Mock {
            current: BiosConfig::Adaptive,
            ignore: true,
            writes: 0,
        };
        let mut j = Log {
            fail: true,
            original: None,
        };
        let cfg = BiosConfig::Custom {
            start: 75,
            stop: 80,
        };
        assert!(apply_verified(&mut m, &mut j, &cfg, Some(B)).is_err());
        assert_eq!(m.writes, 0);
        j.fail = false;
        assert!(apply_verified(&mut m, &mut j, &cfg, Some(B)).is_err());
        assert_eq!(j.original, Some(BiosConfig::Adaptive));
        m.ignore = false;
        assert_eq!(apply_verified(&mut m, &mut j, &cfg, Some(B)).unwrap(), cfg);
        assert_eq!(j.original, Some(BiosConfig::Adaptive));
    }
}
