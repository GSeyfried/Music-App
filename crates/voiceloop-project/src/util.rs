//! Small file-system and identity helpers shared by the crate.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{ProjectError, Result};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Marker in the names of in-progress files. Anything carrying it is safe to delete.
pub const TEMP_MARKER: &str = ".tmp-";

/// A sibling of `path` that no other writer uses: `<name>.tmp-<pid>-<n>`.
pub fn temp_sibling(path: &Path) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    name.push(format!("{TEMP_MARKER}{}-{n}", std::process::id()));
    path.with_file_name(name)
}

/// Flush a directory entry to disk. Best effort: not every platform allows it.
pub fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Move a finished temp file into place. `rename` replaces the destination
/// atomically on POSIX and on Windows (MoveFileEx with replace).
pub fn commit_temp(temp: &Path, path: &Path) -> Result<()> {
    fs::rename(temp, path).map_err(|e| {
        let _ = fs::remove_file(temp);
        ProjectError::io("replace", path, e)
    })?;
    if let Some(parent) = path.parent() {
        sync_dir(parent);
    }
    Ok(())
}

/// Write `bytes` to `path` so that a crash leaves either the old file or the
/// new one, never a partial file: temp file, fsync, rename, fsync directory.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = temp_sibling(path);
    let write = || -> std::io::Result<()> {
        let mut f = File::create(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&temp);
        return Err(ProjectError::io("write", path, e));
    }
    commit_temp(&temp, path)
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// (date `yyyy-mm-dd`, time `hh:mm:ss`) in UTC for seconds since the Unix epoch.
pub fn utc_date_time(unix_seconds: u64) -> (String, String) {
    let (y, m, d) = civil_from_days((unix_seconds / 86_400) as i64);
    let s = unix_seconds % 86_400;
    (format!("{y:04}-{m:02}-{d:02}"), format!("{:02}:{:02}:{:02}", s / 3_600, s / 60 % 60, s % 60))
}

fn unix_now() -> std::time::Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

/// Current time as RFC 3339 UTC, e.g. `2026-11-16T23:06:00Z`.
pub fn now_rfc3339() -> String {
    let (date, time) = utc_date_time(unix_now().as_secs());
    format!("{date}T{time}Z")
}

/// 64-bit FNV-1a.
pub fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A new identifier: 32 hex digits, unique per process and practically unique
/// across machines (time, process id, counter). Not a security token.
pub fn new_id() -> String {
    let now = unix_now();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let a = fnv1a(n, &now.as_nanos().to_le_bytes());
    let b = fnv1a(a, &[std::process::id().to_le_bytes(), (n as u32).to_le_bytes()].concat());
    format!("{a:016x}{b:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_gregorian() {
        assert_eq!(utc_date_time(0), ("1970-01-01".into(), "00:00:00".into()));
        assert_eq!(utc_date_time(951_782_400), ("2000-02-29".into(), "00:00:00".into()));
        assert_eq!(utc_date_time(1_794_870_366), ("2026-11-16".into(), "23:06:06".into()));
        let now = now_rfc3339();
        assert!(now.len() == 20 && now.ends_with('Z') && &now[10..11] == "T");
    }

    #[test]
    fn ids_are_distinct() {
        let a = new_id();
        assert_eq!(a.len(), 32);
        assert_ne!(a, new_id());
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_files() {
        let dir = crate::test_dir("atomic");
        let path = dir.join("a.json");
        atomic_write(&path, b"one").unwrap();
        atomic_write(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        let names: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1);
    }
}
