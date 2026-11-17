use battery::units::{power::watt, ratio::percent, thermodynamic_temperature::degree_celsius};
use chargekeeper_core::{Battery, Snapshot};

#[cfg(windows)]
fn ac_status() -> Option<bool> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: status is a writable, correctly sized OS structure, valid for this call.
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return None;
    }
    match status.ACLineStatus {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}
#[cfg(not(windows))]
fn ac_status() -> Option<bool> {
    None
}

pub fn read() -> Result<Snapshot, String> {
    let manager = battery::Manager::new().map_err(|_| "Windows battery interface unavailable")?;
    let mut batteries = Vec::new();
    for result in manager
        .batteries()
        .map_err(|_| "Cannot enumerate batteries")?
    {
        let b = result
            .map_err(|_| "Battery changed or telemetry unavailable; retrying on next poll")?;
        let charge = f64::from(b.state_of_charge().get::<percent>());
        if !charge.is_finite() || !(0.0..=100.01).contains(&charge) {
            return Err("Battery charge value unavailable".into());
        }
        let state = reported_state(b.state(), charge);
        let rate = f64::from(b.energy_rate().get::<watt>());
        batteries.push(Battery {
            percent: charge.min(100.),
            state: state.into(),
            power_w: reported_rate(rate, state == "discharging"),
            temperature_c: b
                .temperature()
                .map(|t| f64::from(t.get::<degree_celsius>()))
                .filter(|v| v.is_finite()),
            cycles: b.cycle_count(),
            health: Some(f64::from(b.state_of_health().get::<percent>()))
                .filter(|v| v.is_finite() && *v > 0.),
        });
    }
    Ok(Snapshot {
        batteries,
        ac: ac_status(),
        fresh: true,
    })
}

fn reported_state(state: battery::State, charge: f64) -> &'static str {
    match state {
        battery::State::Charging => "charging",
        battery::State::Discharging => "discharging",
        // The dependency uses Full for any idle battery on AC, even at a cap.
        battery::State::Full if charge >= 99.5 => "full",
        battery::State::Empty => "empty",
        _ => "not charging",
    }
}
fn reported_rate(rate: f64, discharging: bool) -> Option<f64> {
    // Windows dependency maps unknown IOCTL rate to zero. Until raw status is
    // exposed, both unknown and measured zero are deliberately shown as missing.
    (rate.is_finite() && rate.abs() > 0.).then_some(if discharging {
        -rate.abs()
    } else {
        rate.abs()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_invented_zero_rate_or_full_state() {
        assert_eq!(reported_rate(0., false), None);
        assert_eq!(reported_rate(f64::NAN, false), None);
        assert_eq!(reported_rate(12., true), Some(-12.));
        assert_eq!(reported_state(battery::State::Full, 80.), "not charging");
        assert_eq!(reported_state(battery::State::Full, 100.), "full");
    }
}
