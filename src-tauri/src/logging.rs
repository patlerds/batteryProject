use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Best-effort bounded local event log. Call sites supply static event names;
/// personal data and raw platform errors never enter the log.
pub fn event(folder: &Path, event: &'static str) {
    let _ = (|| -> std::io::Result<()> {
        fs::create_dir_all(folder)?;
        let path = folder.join("events.jsonl");
        if fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
            let previous = folder.join("events.previous.jsonl");
            if previous.exists() {
                fs::remove_file(&previous)?;
            }
            fs::rename(&path, previous)?;
        }
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut log = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(
            log,
            "{}",
            serde_json::json!({"timestamp": time, "event": event, "version": env!("CARGO_PKG_VERSION")})
        )
    })();
}
