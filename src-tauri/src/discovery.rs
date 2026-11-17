use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    pub manufacturer: String,
    pub model: String,
    pub bios: String,
    pub wmi: bool,
    pub cctk: bool,
    /// `not_checked`, `present`, `absent`, `denied` or `error`; empty if the probe failed.
    #[serde(default)]
    pub wmi_status: String,
    /// Dell's built-in BIOS WMI provider (`root\dcim\sysman\biosattributes`) class
    /// definitions are present. Detection only; says nothing about read or write access.
    #[serde(default)]
    pub dell_bios_provider: bool,
    /// Reading the built-in provider's attribute instances: `not_checked`, `present`,
    /// `absent`, `denied` or `error`; empty if the probe failed.
    #[serde(default)]
    pub dell_bios_read_status: String,
    /// Battery cycle count; `None` when the firmware does not report it (it gives 0).
    #[serde(default)]
    pub cycle_count: Option<u32>,
    /// A battery temperature reading exists.
    #[serde(default)]
    pub temperature_available: bool,
    pub note: String,
}

const STATUSES: [&str; 5] = ["not_checked", "present", "absent", "denied", "error"];

/// Parses the probe's JSON and normalises it: unknown status values become
/// `error` and a cycle count of 0 (firmware "not reported") becomes `None`.
pub fn parse(bytes: &[u8]) -> Result<Discovery, String> {
    let mut d: Discovery =
        serde_json::from_slice(bytes).map_err(|_| "Invalid discovery response".to_string())?;
    for status in [&mut d.wmi_status, &mut d.dell_bios_read_status] {
        if !status.is_empty() && !STATUSES.contains(&status.as_str()) {
            *status = "error".into();
        }
    }
    if d.cycle_count == Some(0) {
        d.cycle_count = None;
    }
    Ok(d)
}

/// Fixed read-only probe; no interpolated arguments, writes, password or generic
/// script endpoint. Output capped and execution bounded. Never run per telemetry poll.
pub fn probe() -> Discovery {
    const SCRIPT: &str = include_str!("../../scripts/discover.ps1");
    let mut command = Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT]);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = (|| -> Result<Discovery, String> {
        let mut child = command
            .spawn()
            .map_err(|_| "Cannot start read-only discovery")?;
        let stdout = child.stdout.take().ok_or("Missing discovery output")?;
        // Drain concurrently to avoid filling the pipe while the process is running.
        let reader = std::thread::spawn(move || {
            let mut data = Vec::new();
            let _ = stdout.take(65_537).read_to_end(&mut data);
            data
        });
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) => {
                    let _ = reader.join();
                    return Err("Windows discovery failed".into());
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err("Cannot wait for discovery".into());
                }
                _ if start.elapsed() > Duration::from_secs(15) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err("Discovery timed out".into());
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        let bytes = reader.join().map_err(|_| "Discovery reader failed")?;
        if bytes.len() > 65_536 {
            return Err("Discovery response too large".into());
        }
        parse(&bytes)
    })();
    result.unwrap_or_else(|note| Discovery {
        note,
        ..Discovery::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_built_in_dell_provider_with_denied_read() {
        let json = br#"{"manufacturer":"Dell Inc.","model":"Latitude 7320 Detachable","bios":"1.50.0","wmi":false,"cctk":false,"wmi_status":"absent","dell_bios_provider":true,"dell_bios_read_status":"denied","cycle_count":null,"temperature_available":false,"note":"n"}"#;
        let d = parse(json).unwrap();
        assert!(!d.wmi);
        assert_eq!(d.wmi_status, "absent");
        assert!(d.dell_bios_provider);
        assert_eq!(d.dell_bios_read_status, "denied");
        assert_eq!(d.cycle_count, None);
        assert!(!d.temperature_available);
    }

    #[test]
    fn legacy_output_without_new_fields_still_parses() {
        let json = br#"{"manufacturer":"Dell Inc.","model":"m","bios":"1","wmi":true,"cctk":false,"wmi_status":"present","note":"n"}"#;
        let d = parse(json).unwrap();
        assert!(d.wmi);
        assert_eq!(d.wmi_status, "present");
        assert!(!d.dell_bios_provider);
        assert_eq!(d.dell_bios_read_status, "");
        assert_eq!(d.cycle_count, None);
        assert!(!d.temperature_available);
    }

    #[test]
    fn zero_cycle_count_is_unavailable() {
        let json = br#"{"manufacturer":"","model":"","bios":"","wmi":false,"cctk":false,"cycle_count":0,"note":""}"#;
        assert_eq!(parse(json).unwrap().cycle_count, None);
        let json = br#"{"manufacturer":"","model":"","bios":"","wmi":false,"cctk":false,"cycle_count":142,"temperature_available":true,"note":""}"#;
        let d = parse(json).unwrap();
        assert_eq!(d.cycle_count, Some(142));
        assert!(d.temperature_available);
    }

    #[test]
    fn unknown_statuses_become_error() {
        let json = br#"{"manufacturer":"","model":"","bios":"","wmi":false,"cctk":false,"wmi_status":"granted","dell_bios_read_status":"yes","note":""}"#;
        let d = parse(json).unwrap();
        assert_eq!(d.wmi_status, "error");
        assert_eq!(d.dell_bios_read_status, "error");
    }

    #[test]
    fn rejects_invalid_or_negative_input() {
        assert!(parse(b"not json").is_err());
        let json = br#"{"manufacturer":"","model":"","bios":"","wmi":false,"cctk":false,"cycle_count":-1,"note":""}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn script_reports_new_fields() {
        let script = include_str!("../../scripts/discover.ps1");
        for field in [
            "dell_bios_provider",
            "dell_bios_read_status",
            "cycle_count",
            "temperature_available",
        ] {
            assert!(script.contains(field), "{field}");
        }
        assert!(!script.contains("Invoke-CimMethod"));
    }
}
