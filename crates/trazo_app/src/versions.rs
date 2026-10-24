//! Earlier versions of a project, kept next to it so work can be recovered.
//!
//! After a save, if the newest copy is older than [`INTERVAL`], the saved file
//! is copied to `<projects>/.versiones/<name>/<unix seconds>.anim`; only the
//! [`KEEP`] newest copies stay.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Minimum time between two automatic copies.
pub const INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Copies kept per project.
pub const KEEP: usize = 20;

pub struct Version {
    pub path: PathBuf,
    pub when: SystemTime,
    pub bytes: u64,
}

/// Folder with the copies of the project at `project`.
pub fn dir_for(project: &Path) -> PathBuf {
    let stem = project.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    project.parent().unwrap_or(Path::new(".")).join(".versiones").join(stem)
}

/// Copies of a project, newest first.
pub fn list(project: &Path) -> Vec<Version> {
    let mut v: Vec<Version> = std::fs::read_dir(dir_for(project))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == trazo_core::io::FILE_EXTENSION))
        .filter_map(|e| {
            let path = e.path();
            let secs: u64 = path.file_stem()?.to_str()?.parse().ok()?;
            Some(Version { when: UNIX_EPOCH + Duration::from_secs(secs), bytes: e.metadata().ok()?.len(), path })
        })
        .collect();
    v.sort_by(|a, b| b.when.cmp(&a.when));
    v
}

/// Replace `dest` with the bytes of `src` without ever leaving it half written.
pub fn replace_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    trazo_core::io::forget_cels(dest);
    let tmp = dest.with_extension("restore.tmp");
    std::fs::copy(src, &tmp)?;
    std::fs::rename(&tmp, dest).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Copy the saved project if the newest copy is old enough (or `force`).
/// Returns whether a copy was made.
pub fn snapshot(project: &Path, force: bool) -> std::io::Result<bool> {
    if !project.is_file() {
        return Ok(false);
    }
    let now = SystemTime::now();
    if !force {
        if let Some(newest) = list(project).first() {
            if now.duration_since(newest.when).unwrap_or_default() < INTERVAL {
                return Ok(false);
            }
        }
    }
    let dir = dir_for(project);
    std::fs::create_dir_all(&dir)?;
    let mut secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // Claim a free name (two threads may snapshot at once; two copies in
    // the same second both stay).
    let target = loop {
        let t = dir.join(format!("{secs}.{}", trazo_core::io::FILE_EXTENSION));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&t) {
            Ok(_) => break t,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => secs += 1,
            Err(e) => return Err(e),
        }
    };
    let tmp = target.with_extension("tmp");
    if let Err(e) = std::fs::copy(project, &tmp).and_then(|_| std::fs::rename(&tmp, &target)) {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    for old in list(project).into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(old.path);
    }
    Ok(true)
}

/// "hace 12 min", "hace 3 h", "hace 2 días".
pub fn age(when: SystemTime) -> String {
    let s = SystemTime::now().duration_since(when).unwrap_or_default().as_secs();
    match s {
        0..60 => crate::i18n::tr("hace un momento").to_string(),
        60..3600 => tf!("hace {} min", s / 60),
        3600..86_400 => tf!("hace {} h", s / 3600),
        _ => {
            let d = s / 86_400;
            if d == 1 { crate::i18n::tr("ayer").to_string() } else { tf!("hace {} días", d) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_spaced_and_pruned() {
        let dir = std::env::temp_dir().join(format!("trazo_versions_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("p.anim");
        std::fs::write(&project, b"uno").unwrap();
        assert!(snapshot(&project, false).unwrap(), "first copy");
        assert!(!snapshot(&project, false).unwrap(), "too soon for another");
        std::fs::write(&project, b"dos").unwrap();
        assert!(snapshot(&project, true).unwrap(), "forced copy");
        let v = list(&project);
        assert_eq!(v.len(), 2);
        assert_eq!(std::fs::read(&v[0].path).unwrap(), b"dos", "newest first");
        for _ in 0..KEEP + 3 {
            snapshot(&project, true).unwrap();
        }
        assert_eq!(list(&project).len(), KEEP);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
