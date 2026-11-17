//! Contract for a future privileged service. No service is installed by this build.
use crate::{dell::Bounds, Capabilities};
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
    pub fn validate(&self, caps: Capabilities, bounds: Option<Bounds>) -> Result<(), String> {
        match self {
            Self::GetStatus {} | Self::RestoreDefaults {} => Ok(()),
            Self::SetLimit { percent } if caps.set_limit && (50..=100).contains(percent) => {
                let b = bounds.ok_or("No device bounds")?;
                let start = percent
                    .checked_sub(b.gap)
                    .ok_or("No valid start threshold")?;
                b.validate(start, *percent)
            }
            Self::SetThresholds { start, stop } if caps.thresholds => {
                bounds.ok_or("No device bounds")?.validate(*start, *stop)
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
            .validate(Capabilities::default(), None)
            .is_err());
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
