use crate::policy::Config;
use std::{fs, io::Read, path::Path};

const MAX_CONFIG_BYTES: usize = 16 * 1024;

pub fn decode_config(bytes: &[u8]) -> Result<Config, String> {
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err("Configuration exceeds the 16 KiB safety limit".into());
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid config JSON")?;
    let object = value.as_object_mut().ok_or("Config must be an object")?;
    let schema = match object.get("schema") {
        None => 0,
        Some(value) => value.as_u64().ok_or("Invalid configuration version")?,
    };
    match schema {
        0 => {
            object.insert("schema".into(), 1.into());
        }
        1 => {}
        _ => return Err("Config is from a newer version; refusing to overwrite it".into()),
    }
    let cfg: Config = serde_json::from_value(value).map_err(|_| "Invalid config fields")?;
    cfg.validate()?;
    Ok(cfg)
}

pub fn load(path: &Path) -> Result<Config, String> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take((MAX_CONFIG_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read config")?;
            decode_config(&bytes)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(_) => Err("Cannot read config".into()),
    }
}

/// A flushed temporary file is renamed on the same volume. Hardware-write
/// journals need stronger ACLs and crash recovery; this is UI preferences only.
pub fn save(path: &Path, cfg: &Config) -> Result<(), String> {
    use std::io::Write;
    cfg.validate()?;
    fs::create_dir_all(path.parent().ok_or("Invalid config path")?)
        .map_err(|_| "Cannot create config folder")?;
    let temporary = path.with_extension("tmp");
    let mut f = fs::File::create(&temporary).map_err(|_| "Cannot stage config")?;
    f.write_all(&serde_json::to_vec_pretty(cfg).map_err(|_| "Cannot serialize config")?)
        .map_err(|_| "Cannot write config")?;
    f.sync_all().map_err(|_| "Cannot flush config")?;
    drop(f);
    fs::rename(&temporary, path).map_err(|_| "Cannot replace config".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrates_legacy_and_rejects_future_or_corrupt() {
        assert_eq!(decode_config(br#"{"limit":85}"#).unwrap().schema, 1);
        for s in [
            r#"{"schema":2}"#,
            r#"{"limit":101}"#,
            "broken",
            "[]",
            r#"{"shell":"cmd"}"#,
            r#"{"schema":-1}"#,
            r#"{"schema":"1"}"#,
            r#"{"schema":null}"#,
        ] {
            assert!(decode_config(s.as_bytes()).is_err());
        }
    }
    #[test]
    fn rejects_oversized_configuration_before_parsing() {
        assert!(decode_config(&vec![b' '; MAX_CONFIG_BYTES + 1]).is_err());
    }
}
