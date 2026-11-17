use crate::policy::Config;
use serde::de::{Deserialize, Deserializer, Error as _, MapAccess, Visitor};
use std::{
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_CONFIG_BYTES: usize = 16 * 1024;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// A top-level JSON object that rejects repeated keys (`serde_json::Value`
/// silently keeps the last one, which would hide a conflicting setting).
struct UniqueObject(serde_json::Map<String, serde_json::Value>);
impl<'de> Deserialize<'de> for UniqueObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = UniqueObject;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<UniqueObject, A::Error> {
                let mut object = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if object.contains_key(&key) {
                        return Err(A::Error::custom(format!("duplicate key `{key}`")));
                    }
                    let value = map.next_value()?;
                    object.insert(key, value);
                }
                Ok(UniqueObject(object))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

pub fn decode_config(bytes: &[u8]) -> Result<Config, String> {
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err("Configuration exceeds the 16 KiB safety limit".into());
    }
    // Editors such as Notepad may save UTF-8 with a byte order mark.
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid config JSON")?;
    if !value.is_object() {
        return Err("Config must be an object".into());
    }
    // Syntax is already known to be valid, so the only failure left is a repeat.
    let UniqueObject(mut object) = serde_json::from_slice(bytes)
        .map_err(|e| format!("Invalid config: {e}; each setting may appear only once"))?;
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
    let cfg: Config = serde_json::from_value(serde_json::Value::Object(object))
        .map_err(|_| "Invalid config fields")?;
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

/// A unique sibling of `path` (same directory, so the rename stays on one
/// volume). Concurrent saves never share or clobber each other's staging file.
fn temporary_path(path: &Path) -> Result<PathBuf, String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .ok_or("Invalid config path")?
        .to_string_lossy();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(path.with_file_name(format!(
        ".{name}.{}.{nanos}.{count}.tmp",
        std::process::id()
    )))
}

/// A flushed (`sync_all`) uniquely named temporary file is renamed over the
/// target on the same volume; the temporary file is removed on any failure.
/// Hardware-write journals need stronger ACLs and crash recovery; this is UI
/// preferences only.
pub fn save(path: &Path, cfg: &Config) -> Result<(), String> {
    use std::io::Write;
    cfg.validate()?;
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|_| "Cannot serialize config")?;
    fs::create_dir_all(path.parent().ok_or("Invalid config path")?)
        .map_err(|_| "Cannot create config folder")?;
    let temporary = temporary_path(path)?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "Cannot stage config")?;
    let staged = f
        .write_all(&bytes)
        .map_err(|_| "Cannot write config")
        .and_then(|()| f.sync_all().map_err(|_| "Cannot flush config"));
    drop(f);
    let result =
        staged.and_then(|()| fs::rename(&temporary, path).map_err(|_| "Cannot replace config"));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
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
    #[test]
    fn accepts_leading_utf8_bom_only() {
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice(br#"{"schema":1,"limit":85}"#);
        assert_eq!(decode_config(&bytes).unwrap().limit, 85);
        // Only a single leading BOM is stripped; anything else stays invalid.
        let mut twice = UTF8_BOM.to_vec();
        twice.extend_from_slice(&bytes);
        assert!(decode_config(&twice).is_err());
        let mut trailing = br#"{"limit":85}"#.to_vec();
        trailing.extend_from_slice(UTF8_BOM);
        assert!(decode_config(&trailing).is_err());
    }
    #[test]
    fn rejects_duplicate_keys_with_a_clear_error() {
        for s in [
            r#"{"limit":85,"limit":60}"#,
            r#"{"schema":1,"schema":1}"#,
            "\u{FEFF}{\"band\":5,\"limit\":80,\"band\":10}",
        ] {
            let err = decode_config(s.as_bytes()).unwrap_err();
            assert!(err.contains("duplicate key"), "{s}: {err}");
        }
        // The other error paths keep their existing messages.
        assert_eq!(
            decode_config(b"[]").unwrap_err(),
            "Config must be an object"
        );
        assert_eq!(decode_config(b"{").unwrap_err(), "Invalid config JSON");
    }
    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "chargekeeper-storage-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn temporary_names_are_unique_siblings() {
        let path = Path::new("dir").join("config.json");
        let a = temporary_path(&path).unwrap();
        let b = temporary_path(&path).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.parent(), path.parent());
        assert_ne!(a, path.with_extension("tmp"));
    }
    #[test]
    fn save_round_trips_replaces_and_leaves_no_temporary_files() {
        let dir = scratch_dir("save");
        let path = dir.join("config.json");
        // A stale file at the old fixed temp name is neither used nor clobbered.
        fs::write(path.with_extension("tmp"), b"stale").unwrap();
        let first = Config {
            limit: 70,
            ..Config::default()
        };
        save(&path, &first).unwrap();
        assert_eq!(load(&path).unwrap().limit, 70);
        let second = Config {
            limit: 90,
            ..Config::default()
        };
        save(&path, &second).unwrap();
        assert_eq!(load(&path).unwrap().limit, 90);
        assert_eq!(fs::read(path.with_extension("tmp")).unwrap(), b"stale");
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["config.json", "config.tmp"]);
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn failed_replace_cleans_up_the_temporary_file() {
        let dir = scratch_dir("fail");
        // A directory at the target path makes the final rename fail.
        let path = dir.join("config.json");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), b"x").unwrap();
        assert_eq!(
            save(&path, &Config::default()).unwrap_err(),
            "Cannot replace config"
        );
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["config.json"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
