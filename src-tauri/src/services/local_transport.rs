//! Preparation only: consumers must opt into this policy in the enforcement slice.

use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static SERIAL: AtomicU64 = AtomicU64::new(0);

// Only registered on Linux/macOS: no assumption about other Unix ABIs.
extern "C" {
    fn geteuid() -> u32;
    fn flock(fd: i32, operation: i32) -> i32;
}

pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments and no preconditions on these targets.
    unsafe { geteuid() }
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "unsafe local transport endpoint",
    )
}

fn check_private(m: &Metadata, uid: u32) -> io::Result<()> {
    if !m.is_dir() || m.uid() != uid || m.mode() & 0o7777 != 0o700 {
        return Err(denied());
    }
    Ok(())
}

fn validate_chain(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(denied());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => current.push(component),
            _ => return Err(denied()),
        }
        let m = fs::symlink_metadata(&current)?;
        if !m.is_dir() || (m.uid() != 0 && m.uid() != effective_uid()) {
            return Err(denied());
        }
        // A root-owned sticky shared temp root is allowed, not a user-owned
        // writable ancestor. Descendants are still validated independently.
        if m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & 0o1000 != 0) {
            return Err(denied());
        }
    }
    Ok(())
}

/// Canonical root is deliberately independent of USER, UID and TMPDIR.
pub fn runtime_dir(create: bool) -> io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    let root = Path::new("/tmp");
    #[cfg(target_os = "macos")]
    let root = Path::new("/private/tmp");
    resolve_at(root, create)
}

/// `create=false` is the CLI contract: never create or repair filesystem state.
pub fn resolve_at(root: &Path, create: bool) -> io::Result<PathBuf> {
    validate_chain(root)?;
    let private = root.join(format!("cadence-{}", effective_uid()));
    if create {
        match DirBuilder::new().mode(0o700).create(&private) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    check_private(&fs::symlink_metadata(&private)?, effective_uid())?;
    Ok(private)
}

fn private_parent(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(denied)?;
    validate_chain(parent)?;
    check_private(&fs::symlink_metadata(parent)?, effective_uid())
}

/// Validate before connecting; never follow a socket symlink.
pub fn validate_socket(path: &Path) -> io::Result<Metadata> {
    let m = socket_metadata(path)?;
    if m.mode() & 0o7777 != 0o600 {
        return Err(denied());
    }
    Ok(m)
}

fn socket_metadata(path: &Path) -> io::Result<Metadata> {
    private_parent(path)?;
    let m = fs::symlink_metadata(path)?;
    if !m.file_type().is_socket() || m.uid() != effective_uid() {
        return Err(denied());
    }
    Ok(m)
}

fn same_inode(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// A lock is never unlinked. Closing its File releases flock automatically.
pub struct EndpointLease {
    path: PathBuf,
    _lock: File,
}

impl EndpointLease {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        private_parent(path)?;
        let lock_path = path.with_extension("lock");
        let lock = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let before = fs::symlink_metadata(&lock_path)?;
                if !before.is_file()
                    || before.uid() != effective_uid()
                    || before.mode() & 0o7777 != 0o600
                {
                    return Err(denied());
                }
                let file = OpenOptions::new().write(true).open(&lock_path)?;
                if !same_inode(&before, &file.metadata()?) {
                    return Err(denied());
                }
                file
            }
            Err(e) => return Err(e),
        };
        // SAFETY: the File owns a valid fd; LOCK_EX | LOCK_NB is 2 | 4
        // on the explicitly supported Linux/macOS targets. No blocking wait.
        if unsafe { flock(lock.as_raw_fd(), 2 | 4) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            path: path.to_owned(),
            _lock: lock,
        })
    }

    /// Only a validated, refused socket may be reclaimed under the held lock.
    pub async fn bind(&self) -> io::Result<tokio::net::UnixListener> {
        private_parent(&self.path)?;
        match fs::symlink_metadata(&self.path) {
            Ok(_) => {
                let before = validate_socket(&self.path)?;
                let probe = tokio::time::timeout(
                    Duration::from_millis(200),
                    tokio::net::UnixStream::connect(&self.path),
                )
                .await;
                match probe {
                    Ok(Err(e)) if e.kind() == io::ErrorKind::ConnectionRefused => {}
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            "local endpoint is live or cannot be safely reclaimed",
                        ))
                    }
                }
                if !same_inode(&before, &validate_socket(&self.path)?) {
                    return Err(denied());
                }
                fs::remove_file(&self.path)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = tokio::net::UnixListener::bind(&self.path)?;
        self.secure_socket()?;
        Ok(listener)
    }

    /// For an owned mpv Child's newly created socket, before connecting.
    pub fn secure_socket(&self) -> io::Result<()> {
        let before = socket_metadata(&self.path)?;
        if !same_inode(&before, &socket_metadata(&self.path)?) {
            return Err(denied());
        }
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        validate_socket(&self.path).map(|_| ())
    }

    /// Caller must first stop its actual owned Child/listener. No PID lookup.
    pub fn remove_socket(&self) -> io::Result<()> {
        match validate_socket(&self.path) {
            Ok(before) => {
                if !same_inode(&before, &validate_socket(&self.path)?) {
                    return Err(denied());
                }
                fs::remove_file(&self.path)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Fresh directory per mpv launch, including supervisor restarts. Never reuse
/// an old PID or socket; the caller retains and shuts down only its owned Child.
pub fn session_socket(root: &Path) -> io::Result<PathBuf> {
    validate_chain(root)?;
    check_private(&fs::symlink_metadata(root)?, effective_uid())?;
    for _ in 0..32 {
        let name = format!(
            "mpv-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        );
        let dir = root.join(name);
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {
                check_private(&fs::symlink_metadata(&dir)?, effective_uid())?;
                return Ok(dir.join("ipc.sock"));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "session name collision limit",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cadence-policy-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn resolver_is_private_and_client_is_read_only() {
        let _canonical_resolver: fn(bool) -> io::Result<PathBuf> = runtime_dir;
        let f = Fixture::new();
        assert!(resolve_at(&f.0, false).is_err());
        assert_eq!(std::fs::read_dir(&f.0).unwrap().count(), 0);
        let server = resolve_at(&f.0, true).unwrap();
        assert_eq!(server, resolve_at(&f.0, false).unwrap());
        assert_eq!(
            server.file_name().unwrap(),
            format!("cadence-{}", effective_uid()).as_str()
        );
        let m = std::fs::symlink_metadata(server).unwrap();
        assert_eq!(m.uid(), effective_uid());
        assert_eq!(m.mode() & 0o777, 0o700);
        // Identity has no USER/UID input: spoofed labels cannot affect resolution.
        assert!(!include_str!("local_transport.rs").contains("var(\"USER\")"));
        assert!(!include_str!("local_transport.rs").contains("var(\"UID\")"));
    }

    #[test]
    fn rejects_untrusted_components_without_repair() {
        let f = Fixture::new();
        assert!(resolve_at(Path::new("relative"), true).is_err());
        assert!(resolve_at(&f.0.join(".."), true).is_err());
        let private = f.0.join(format!("cadence-{}", effective_uid()));
        std::fs::write(&private, b"untouched").unwrap();
        assert!(resolve_at(&f.0, true).is_err());
        assert_eq!(std::fs::read(&private).unwrap(), b"untouched");
        std::fs::remove_file(&private).unwrap();
        symlink(&f.0, &private).unwrap();
        assert!(resolve_at(&f.0, true).is_err());
        std::fs::remove_file(&private).unwrap();
        std::fs::create_dir(&private).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(resolve_at(&f.0, true).is_err());
        assert_eq!(std::fs::metadata(&private).unwrap().mode() & 0o777, 0o777);
        let mut metadata = std::fs::metadata(&f.0).unwrap();
        assert!(check_private(&metadata, effective_uid().wrapping_add(1)).is_err());
        std::fs::set_permissions(&f.0, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(resolve_at(&f.0, true).is_err());
        std::fs::set_permissions(&f.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        metadata = std::fs::metadata(&f.0).unwrap();
        assert!(check_private(&metadata, effective_uid()).is_ok());
    }

    #[tokio::test]
    async fn lock_live_stale_and_file_collisions_preserve_identity() {
        let f = Fixture::new();
        let root = resolve_at(&f.0, true).unwrap();
        let endpoint = root.join("control.sock");
        let lease = EndpointLease::acquire(&endpoint).unwrap();
        assert!(EndpointLease::acquire(&endpoint).is_err());
        std::fs::write(&endpoint, b"keep").unwrap();
        assert!(lease.bind().await.is_err());
        assert_eq!(std::fs::read(&endpoint).unwrap(), b"keep");
        std::fs::remove_file(&endpoint).unwrap();
        let live = lease.bind().await.unwrap();
        assert_eq!(std::fs::metadata(&endpoint).unwrap().mode() & 0o777, 0o600);
        validate_socket(&endpoint).unwrap(); // CLI validation and server share policy.
        std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(validate_socket(&endpoint).is_err());
        assert!(lease.bind().await.is_err());
        std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(lease.bind().await.is_err());
        assert!(endpoint.exists());
        drop(live);
        let recovered = lease.bind().await.unwrap();
        drop(recovered);
        lease.remove_socket().unwrap();
        assert!(!endpoint.exists());
        drop(lease);
        assert!(EndpointLease::acquire(&endpoint).is_ok());
    }

    #[test]
    fn lock_symlink_and_nonregular_files_are_rejected() {
        let f = Fixture::new();
        let root = resolve_at(&f.0, true).unwrap();
        let endpoint = root.join("control.sock");
        let lock = endpoint.with_extension("lock");
        symlink(&f.0, &lock).unwrap();
        assert!(EndpointLease::acquire(&endpoint).is_err());
        std::fs::remove_file(&lock).unwrap();
        std::fs::create_dir(&lock).unwrap();
        assert!(EndpointLease::acquire(&endpoint).is_err());
        std::fs::remove_dir(&lock).unwrap();
        std::fs::write(&lock, b"keep").unwrap();
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(EndpointLease::acquire(&endpoint).is_err());
        assert_eq!(std::fs::read(&lock).unwrap(), b"keep");
        symlink(&lock, &endpoint).unwrap();
        assert!(validate_socket(&endpoint).is_err());
    }

    #[test]
    fn sessions_are_fresh_and_cleanup_never_uses_pid_files() {
        let f = Fixture::new();
        let root = resolve_at(&f.0, true).unwrap();
        let a = session_socket(&root).unwrap();
        let b = session_socket(&root).unwrap();
        assert_ne!(a, b);
        assert!(!a.exists());
        assert_eq!(
            std::fs::metadata(a.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        std::fs::write(a.with_extension("pid"), b"not a process identifier").unwrap();
        let lease = EndpointLease::acquire(&a).unwrap();
        assert!(lease.remove_socket().is_ok());
        assert_eq!(
            std::fs::read(a.with_extension("pid")).unwrap(),
            b"not a process identifier"
        );
        std::fs::write(&a, b"not a socket").unwrap();
        assert!(lease.remove_socket().is_err());
        assert_eq!(std::fs::read(&a).unwrap(), b"not a socket");
    }
}
