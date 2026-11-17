//! Contract for a future privileged service. No service is installed by this build.
use crate::{dell::Bounds, policy::SAFETY_FLOOR, Capabilities};
use serde::{Deserialize, Serialize};
pub const MAX_MESSAGE: usize = 4096;
pub const LEASE_SECONDS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    GetStatus {},
    SetLimit { percent: u8 },
    SetThresholds { start: u8, stop: u8 },
    SetChargeInhibit { enabled: bool },
    ForceDischarge { enabled: bool },
    RestoreDefaults {},
}
impl Request {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_MESSAGE {
            return Err("Message exceeds 4096 bytes".into());
        }
        serde_json::from_slice(bytes).map_err(|_| "Invalid request".into())
    }
    /// `lowest_percent` is the service's own latest reading of the lowest pack.
    /// Enabling inhibit or force-discharge is refused at or below the safety
    /// floor, or when that reading is unknown. Disabling is always allowed.
    /// The service must still re-check the floor on every tick while enabled.
    pub fn validate(
        &self,
        caps: Capabilities,
        bounds: Option<Bounds>,
        lowest_percent: Option<f64>,
    ) -> Result<(), String> {
        let above_floor =
            lowest_percent.is_some_and(|p| p.is_finite() && p > SAFETY_FLOOR && p <= 100.0);
        match self {
            Self::GetStatus {} | Self::RestoreDefaults {} => Ok(()),
            Self::SetLimit { percent } if caps.set_limit && (50..=100).contains(percent) => {
                let b = bounds.ok_or("No device bounds")?.effective()?;
                let start = percent
                    .checked_sub(b.gap)
                    .ok_or("No valid start threshold")?;
                b.validate(start, *percent)
            }
            Self::SetThresholds { start, stop } if caps.thresholds => {
                bounds.ok_or("No device bounds")?.validate(*start, *stop)
            }
            Self::SetChargeInhibit { enabled } | Self::ForceDischarge { enabled }
                if *enabled && !above_floor =>
            {
                Err("Refused: battery at or below the safety floor, or level unknown".into())
            }
            Self::SetChargeInhibit { .. } if caps.inhibit => Ok(()),
            Self::ForceDischarge { .. } if caps.force_discharge => Ok(()),
            _ => Err("Unsupported operation or invalid input".into()),
        }
    }
}

#[derive(Default)]
pub struct Lease {
    last: Option<u64>,
}
impl Lease {
    /// Transport must authenticate owner identity before renewing. Never trust a
    /// timestamp supplied by the client; pass the service's monotonic clock.
    pub fn renew_authenticated(&mut self, now: u64) {
        self.last = Some(now);
    }
    pub fn expired(&self, now: u64) -> bool {
        self.last
            .is_none_or(|last| now < last || now - last >= LEASE_SECONDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_untrusted_inputs() {
        for input in [
            r#"{"command":"set_limit","percent":256}"#,
            r#"{"command":"get_status","key":"BCLM"}"#,
            r#"{"command":"exec","path":"cmd.exe"}"#,
        ] {
            assert!(
                Request::decode(input.as_bytes()).is_err(),
                "Unexpectedly accepted {input}"
            );
        }
        assert!(Request::decode(&vec![b' '; MAX_MESSAGE + 1]).is_err());
        assert!(Request::SetLimit { percent: 80 }
            .validate(Capabilities::default(), None, Some(60.))
            .is_err());
    }
    #[test]
    fn primitives_refused_at_or_below_floor() {
        let caps = Capabilities {
            set_limit: true,
            thresholds: true,
            inhibit: true,
            force_discharge: true,
            temperature: true,
        };
        for enabled_request in [
            Request::ForceDischarge { enabled: true },
            Request::SetChargeInhibit { enabled: true },
        ] {
            for lowest in [None, Some(20.), Some(5.), Some(f64::NAN)] {
                assert!(enabled_request.validate(caps, None, lowest).is_err());
            }
            assert!(enabled_request.validate(caps, None, Some(60.)).is_ok());
        }
        for off in [
            Request::ForceDischarge { enabled: false },
            Request::SetChargeInhibit { enabled: false },
        ] {
            assert!(off.validate(caps, None, None).is_ok());
        }
        let wide = Bounds {
            start_min: 0,
            start_max: 100,
            stop_min: 0,
            stop_max: 100,
            gap: 1,
        };
        assert!(Request::SetLimit { percent: 52 }
            .validate(caps, Some(wide), Some(60.))
            .is_err());
        assert!(Request::SetLimit { percent: 80 }
            .validate(caps, Some(wide), Some(60.))
            .is_ok());
    }
    #[test]
    fn watchdog_expires_without_renewal() {
        let mut l = Lease::default();
        assert!(l.expired(0));
        l.renew_authenticated(5);
        assert!(!l.expired(34));
        assert!(l.expired(35));
        assert!(l.expired(4));
    }
}
