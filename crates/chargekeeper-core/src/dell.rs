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
/// Published Dell ranges for `PrimaryBattChargeCfg=Custom:start-stop` (see
/// docs/PLAN.md): start 50–95, stop 55–100, gap >= 5. These are a hard outer
/// limit: device-discovered bounds can only narrow them, never widen them.
pub const DOCUMENTED_BOUNDS: Bounds = Bounds {
    start_min: 50,
    start_max: 95,
    stop_min: 55,
    stop_max: 100,
    gap: 5,
};
impl Bounds {
    /// Discovered bounds intersected with `DOCUMENTED_BOUNDS`.
    pub fn effective(&self) -> Result<Bounds, String> {
        if self.start_min > self.start_max
            || self.stop_min > self.stop_max
            || self.start_max > 100
            || self.stop_max > 100
            || self.gap == 0
            || self.gap > 100
        {
            return Err("Invalid device-reported bounds".into());
        }
        let d = DOCUMENTED_BOUNDS;
        let b = Bounds {
            start_min: self.start_min.max(d.start_min),
            start_max: self.start_max.min(d.start_max),
            stop_min: self.stop_min.max(d.stop_min),
            stop_max: self.stop_max.min(d.stop_max),
            gap: self.gap.max(d.gap),
        };
        if b.start_min > b.start_max || b.stop_min > b.stop_max {
            return Err("Device bounds fall outside the documented Dell ranges".into());
        }
        Ok(b)
    }
    pub fn validate(&self, start: u8, stop: u8) -> Result<(), String> {
        let b = self.effective()?;
        if !(b.start_min..=b.start_max).contains(&start)
            || !(b.stop_min..=b.stop_max).contains(&stop)
            || stop <= start
            || stop - start < b.gap
        {
            return Err("Thresholds outside device bounds or minimum gap".into());
        }
        Ok(())
    }
}

/// The single (limit, band) -> (start, stop) mapping: start = limit - band.
pub fn thresholds_for(limit: u8, band: u8, bounds: Bounds) -> Result<(u8, u8), String> {
    let start = limit.checked_sub(band).ok_or("No valid start threshold")?;
    bounds.validate(start, limit)?;
    Ok((start, limit))
}

/// Strict percentage: 1–3 ASCII digits, no sign, no leading zero.
fn parse_percent(s: &str) -> Option<u8> {
    let digits = !s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
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
            let start = parse_percent(start).ok_or("Invalid start threshold")?;
            let stop = parse_percent(stop).ok_or("Invalid stop threshold")?;
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
    /// The user's original state, if one has already been recorded.
    fn recorded_original(&mut self) -> Result<Option<BiosConfig>, String>;
    /// Implementation must durably persist original state BEFORE returning.
    /// `apply_verified` calls this only when `recorded_original` is `None`
    /// (first write wins), so Chargekeeper's own values never replace it.
    fn record_original(&mut self, original: &BiosConfig) -> Result<(), String>;
}
fn write_and_verify(
    transport: &mut impl DellTransport,
    config: &BiosConfig,
) -> Result<BiosConfig, String> {
    transport.write(config)?;
    let actual = transport.read()?;
    if actual != *config {
        return Err("BIOS readback mismatch; enforcement is unconfirmed".into());
    }
    Ok(actual)
}
pub fn apply_verified(
    transport: &mut impl DellTransport,
    journal: &mut impl Journal,
    config: &BiosConfig,
    bounds: Option<Bounds>,
) -> Result<BiosConfig, String> {
    cctk_args(config, bounds)?;
    let previous = transport.read()?;
    if journal.recorded_original()?.is_none() {
        journal.record_original(&previous)?;
    }
    let error = match write_and_verify(transport, config) {
        Ok(actual) => return Ok(actual),
        Err(error) => error,
    };
    // Roll back to the pre-write state, but never write a value that fails the
    // documented/discovered bounds (e.g. an out-of-range Custom read earlier).
    let rollback =
        cctk_args(&previous, bounds).and_then(|_| write_and_verify(transport, &previous));
    Err(match rollback {
        Ok(_) => format!("{error}; previous BIOS setting restored"),
        Err(e) => format!("{error}; ROLLBACK FAILED: {e}"),
    })
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
    #[test]
    fn discovered_bounds_cannot_widen_documented_ranges() {
        let wide = Bounds {
            start_min: 0,
            start_max: 100,
            stop_min: 0,
            stop_max: 100,
            gap: 1,
        };
        assert_eq!(wide.effective().unwrap(), DOCUMENTED_BOUNDS);
        for (start, stop) in [(0, 1), (20, 40), (49, 80), (78, 80), (96, 100)] {
            assert!(wide.validate(start, stop).is_err(), "{start}-{stop}");
        }
        assert!(cctk_args(&BiosConfig::Custom { start: 0, stop: 1 }, Some(wide)).is_err());
        assert!(wide.validate(50, 55).is_ok());
        let narrow = Bounds {
            start_min: 60,
            start_max: 85,
            stop_min: 70,
            stop_max: 90,
            gap: 10,
        };
        assert!(narrow.validate(50, 80).is_err());
        assert!(narrow.validate(75, 80).is_err());
        assert!(narrow.validate(70, 80).is_ok());
        let disjoint = Bounds {
            start_min: 10,
            start_max: 40,
            ..DOCUMENTED_BOUNDS
        };
        assert!(disjoint.effective().is_err());
        assert_eq!(thresholds_for(80, 5, B), Ok((75, 80)));
        assert!(thresholds_for(80, 2, B).is_err());
        assert!(thresholds_for(60, 20, B).is_err());
    }
    #[test]
    fn strict_threshold_digits() {
        for value in [
            "PrimaryBattChargeCfg=Custom:+75-80",
            "PrimaryBattChargeCfg=Custom:075-080",
            "PrimaryBattChargeCfg=Custom:75-+80",
            "PrimaryBattChargeCfg=Custom: 75-80",
            "PrimaryBattChargeCfg=Custom:75-1000",
        ] {
            assert!(parse_cctk(value).is_err(), "{value}");
        }
    }
    struct Mock {
        current: BiosConfig,
        ignore: bool,
        writes: Vec<BiosConfig>,
        /// Next write lands as this value instead (firmware substitution).
        substitute: Option<BiosConfig>,
        /// Next write partially applies `substitute` and then errors.
        fail_next_write: bool,
    }
    impl Mock {
        fn new(current: BiosConfig) -> Self {
            Self {
                current,
                ignore: false,
                writes: Vec::new(),
                substitute: None,
                fail_next_write: false,
            }
        }
    }
    impl DellTransport for Mock {
        fn read(&mut self) -> Result<BiosConfig, String> {
            Ok(self.current.clone())
        }
        fn write(&mut self, c: &BiosConfig) -> Result<(), String> {
            self.writes.push(c.clone());
            if let Some(s) = self.substitute.take() {
                self.current = s;
            } else if !self.ignore {
                self.current = c.clone();
            }
            if std::mem::take(&mut self.fail_next_write) {
                return Err("cctk exited with an error".into());
            }
            Ok(())
        }
    }
    /// Deliberately naive: overwrites on every call, so it cannot hide a
    /// caller that records Chargekeeper's own value as the "original".
    struct Log {
        fail: bool,
        original: Option<BiosConfig>,
        records: u8,
    }
    impl Journal for Log {
        fn recorded_original(&mut self) -> Result<Option<BiosConfig>, String> {
            Ok(self.original.clone())
        }
        fn record_original(&mut self, c: &BiosConfig) -> Result<(), String> {
            if self.fail {
                return Err("Disk failure".into());
            }
            self.records += 1;
            self.original = Some(c.clone());
            Ok(())
        }
    }
    fn log() -> Log {
        Log {
            fail: false,
            original: None,
            records: 0,
        }
    }
    const CFG: BiosConfig = BiosConfig::Custom {
        start: 75,
        stop: 80,
    };
    #[test]
    fn journal_failure_prevents_write_and_readback_failure_is_error() {
        let mut m = Mock::new(BiosConfig::Adaptive);
        m.ignore = true;
        let mut j = Log {
            fail: true,
            ..log()
        };
        assert!(apply_verified(&mut m, &mut j, &CFG, Some(B)).is_err());
        assert!(m.writes.is_empty());
        j.fail = false;
        assert!(apply_verified(&mut m, &mut j, &CFG, Some(B)).is_err());
        assert_eq!(j.original, Some(BiosConfig::Adaptive));
        m.ignore = false;
        assert_eq!(apply_verified(&mut m, &mut j, &CFG, Some(B)).unwrap(), CFG);
        assert_eq!(j.original, Some(BiosConfig::Adaptive));
    }
    #[test]
    fn first_recorded_original_wins_across_applies() {
        let mut m = Mock::new(BiosConfig::Adaptive);
        let mut j = log();
        assert_eq!(apply_verified(&mut m, &mut j, &CFG, Some(B)).unwrap(), CFG);
        let next = BiosConfig::Custom {
            start: 60,
            stop: 70,
        };
        assert_eq!(
            apply_verified(&mut m, &mut j, &next, Some(B)).unwrap(),
            next
        );
        assert_eq!(apply_verified(&mut m, &mut j, &CFG, Some(B)).unwrap(), CFG);
        assert_eq!(j.original, Some(BiosConfig::Adaptive));
        assert_eq!(j.records, 1);
    }
    #[test]
    fn readback_mismatch_rolls_back_to_previous_state() {
        let mut m = Mock::new(BiosConfig::Express);
        m.substitute = Some(BiosConfig::Standard);
        let e = apply_verified(&mut m, &mut log(), &CFG, Some(B)).unwrap_err();
        assert!(e.contains("mismatch") && e.contains("restored"), "{e}");
        assert_eq!(m.writes, [CFG, BiosConfig::Express]);
        assert_eq!(m.current, BiosConfig::Express);
    }
    #[test]
    fn write_error_rolls_back_and_reports_failed_rollback() {
        let mut m = Mock::new(BiosConfig::Adaptive);
        m.substitute = Some(BiosConfig::Standard);
        m.fail_next_write = true;
        let e = apply_verified(&mut m, &mut log(), &CFG, Some(B)).unwrap_err();
        assert!(e.contains("restored"), "{e}");
        assert_eq!(m.current, BiosConfig::Adaptive);
        // Rollback that does not take is reported, never hidden.
        let mut m = Mock::new(BiosConfig::Adaptive);
        m.ignore = true;
        let e = apply_verified(&mut m, &mut log(), &CFG, Some(B)).unwrap_err();
        assert!(e.contains("mismatch"), "{e}");
        // ignore=true: current stays Adaptive, so rollback readback matches.
        let mut m = Mock::new(BiosConfig::Adaptive);
        m.substitute = Some(BiosConfig::Standard);
        m.ignore = true;
        let e = apply_verified(&mut m, &mut log(), &CFG, Some(B)).unwrap_err();
        assert!(e.contains("ROLLBACK FAILED"), "{e}");
        assert_eq!(m.current, BiosConfig::Standard);
    }
    #[test]
    fn out_of_bounds_previous_state_is_never_written_back() {
        let bad = BiosConfig::Custom { start: 0, stop: 1 };
        let mut m = Mock::new(bad.clone());
        m.substitute = Some(BiosConfig::Standard);
        let e = apply_verified(&mut m, &mut log(), &CFG, Some(B)).unwrap_err();
        assert!(e.contains("ROLLBACK FAILED"), "{e}");
        assert_eq!(m.writes, [CFG]);
    }
}
