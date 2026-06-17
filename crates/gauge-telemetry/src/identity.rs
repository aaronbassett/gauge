//! Install identity: a random v4 UUID persisted `0600`, created race-safely and
//! reused thereafter. The session UUID is minted per process and never stored.

use std::io;
use std::path::Path;
use std::time::SystemTime;

use uuid::Uuid;

/// Load the install UUID, creating it on first run. Race-safe: a concurrent
/// process either creates it or loses the race and reads the winner's value.
pub fn load_or_create(path: &Path) -> io::Result<Uuid> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    match opts.open(path) {
        Ok(mut file) => {
            // Write through the exclusive handle so the file is never
            // observable as empty by a concurrent reader that lost the race.
            use std::io::Write as _;
            let id = Uuid::new_v4();
            file.write_all(id.to_string().as_bytes())?;
            Ok(id)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match read(path) {
            Ok(id) => Ok(id),
            // Corrupt/partial file (e.g. crash mid-write): regenerate rather
            // than wedge `build()` forever.
            Err(e) if e.kind() == io::ErrorKind::InvalidData => reset(path),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    }
}

fn read(path: &Path) -> io::Result<Uuid> {
    let s = std::fs::read_to_string(path)?;
    Uuid::parse_str(s.trim()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Regenerate the install UUID (severing future continuity). Overwrites in place.
pub fn reset(path: &Path) -> io::Result<Uuid> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let id = Uuid::new_v4();
    std::fs::write(path, id.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(id)
}

/// The install file's mtime = mint time, used for the first-run grace period.
pub fn mint_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_reuse_is_stable() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("sub/id");
        let a = load_or_create(&p).unwrap();
        let b = load_or_create(&p).unwrap();
        assert_eq!(a, b, "second call reuses the persisted UUID");
        assert!(mint_time(&p).is_some());
    }

    #[test]
    fn corrupt_file_self_heals() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("id");
        std::fs::write(&p, "not-a-uuid").unwrap();
        let healed = load_or_create(&p).unwrap();
        // File now holds the regenerated, parseable UUID.
        let on_disk = std::fs::read_to_string(&p).unwrap();
        assert_eq!(Uuid::parse_str(on_disk.trim()).unwrap(), healed);
        // And it is stable on the next load.
        assert_eq!(healed, load_or_create(&p).unwrap());
    }

    #[test]
    fn reset_changes_the_uuid() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("id");
        let a = load_or_create(&p).unwrap();
        let b = reset(&p).unwrap();
        assert_ne!(a, b);
        assert_eq!(b, load_or_create(&p).unwrap(), "reset value persists");
    }

    #[cfg(unix)]
    #[test]
    fn file_is_0600() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("id");
        load_or_create(&p).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
