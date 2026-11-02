//! **The box guard** — how full the data disk is. Free-tier writes are
//! refused `box_full` once it passes the plan's full line (85 %); admins
//! are warned past its warn line (70 %), in `quota.table` and the log
//! (V2-31, the owner's free tier). The reading is behind [`DiskUsage`], so a
//! test gives a fake one.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How full a disk is.
pub trait DiskUsage: Send + Sync + std::fmt::Debug {
    /// Percent used, 0 to 100; `None` when it cannot be read (then nothing is refused).
    fn used_percent(&self) -> Option<f64>;
}

/// The disk holding `path`, read by statvfs(3) — used as `df` counts it
/// (blocks not free to an unprivileged writer count as used) — at most once
/// every few seconds.
#[derive(Debug)]
pub struct Statvfs {
    path: PathBuf,
    last: Mutex<Option<(Instant, Option<f64>)>>,
}

impl Statvfs {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self { path: path.as_ref().to_path_buf(), last: Mutex::new(None) }
    }

    fn read(&self) -> Option<f64> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(self.path.as_os_str().as_bytes()).ok()?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: a valid C string and a zeroed struct to fill.
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
            return None;
        }
        let (blocks, free, avail) = (s.f_blocks as f64, s.f_bfree as f64, s.f_bavail as f64);
        let used = blocks - free;
        let usable = used + avail;
        (usable > 0.0).then(|| (used / usable * 100.0).clamp(0.0, 100.0))
    }
}

impl DiskUsage for Statvfs {
    fn used_percent(&self) -> Option<f64> {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, v)) = *last {
            if at.elapsed() < Duration::from_secs(5) {
                return v;
            }
        }
        let v = self.read();
        *last = Some((Instant::now(), v));
        v
    }
}

/// A disk as full as a test says (`set`).
#[derive(Debug, Default)]
pub struct FakeDisk(Mutex<Option<f64>>);

impl FakeDisk {
    pub fn at(percent: f64) -> Self {
        Self(Mutex::new(Some(percent)))
    }

    pub fn set(&self, percent: f64) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(percent);
    }
}

impl DiskUsage for FakeDisk {
    fn used_percent(&self) -> Option<f64> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Where the box stands against the plan's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BoxState {
    Ok,
    Warn,
    Full,
    Unknown,
}

pub fn state(used: Option<f64>, lines: crate::plan::BoxLines) -> BoxState {
    match used {
        None => BoxState::Unknown,
        Some(u) if u >= lines.full_percent => BoxState::Full,
        Some(u) if u >= lines.warn_percent => BoxState::Warn,
        Some(_) => BoxState::Ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::BoxLines;

    #[test]
    fn the_lines_are_the_plans() {
        let l = BoxLines::default();
        assert_eq!(state(Some(10.0), l), BoxState::Ok);
        assert_eq!(state(Some(70.0), l), BoxState::Warn);
        assert_eq!(state(Some(84.9), l), BoxState::Warn);
        assert_eq!(state(Some(85.0), l), BoxState::Full);
        assert_eq!(state(None, l), BoxState::Unknown);
    }

    #[test]
    fn this_machines_disk_reads() {
        let used = Statvfs::new(std::env::temp_dir()).used_percent().expect("statvfs on the temp dir");
        assert!((0.0..=100.0).contains(&used), "{used}");
    }
}
