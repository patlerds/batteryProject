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
    pub note: String,
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
        serde_json::from_slice(&bytes).map_err(|_| "Invalid discovery response".into())
    })();
    result.unwrap_or_else(|note| Discovery {
        note,
        ..Discovery::default()
    })
}
