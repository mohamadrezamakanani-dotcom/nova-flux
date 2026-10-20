use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::Path,
};

#[derive(Debug)]
pub enum Acquisition {
    Acquired(InstanceGuard),
    AlreadyRunning,
}

#[derive(Debug)]
pub struct InstanceGuard {
    // Closing this handle releases the OS lock, including on process exit/crash.
    // Never unlink the file: another process may already have opened its inode.
    _file: File,
}

pub fn acquire() -> io::Result<Acquisition> {
    let path = nova_flux::platform::data_dir()?.join("gui.lock");
    acquire_at(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "Cannot secure the GUI instance lock at {}: {error}",
                path.display()
            ),
        )
    })
}

fn acquire_at(path: &Path) -> io::Result<Acquisition> {
    if let Some(parent) = path.parent() {
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(parent)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Acquisition::Acquired(InstanceGuard { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(Acquisition::AlreadyRunning),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(target_os = "macos")]
pub fn activate_existing() -> bool {
    macos::activate_existing()
}

#[cfg(not(target_os = "macos"))]
pub fn activate_existing() -> bool {
    false
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void, CStr};

    type Object = *mut c_void;
    type Selector = *const c_void;
    #[cfg(target_arch = "aarch64")]
    type ObjcBool = bool;
    #[cfg(not(target_arch = "aarch64"))]
    type ObjcBool = i8;

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Object;
        fn sel_registerName(name: *const c_char) -> Selector;
        fn objc_msgSend();
        fn objc_autoreleasePoolPush() -> Object;
        fn objc_autoreleasePoolPop(pool: Object);
    }

    struct AutoreleasePool(Object);

    impl Drop for AutoreleasePool {
        fn drop(&mut self) {
            // SAFETY: This pool was pushed on this thread and owns one pop.
            unsafe { objc_autoreleasePoolPop(self.0) };
        }
    }

    pub(super) fn activate_existing() -> bool {
        // SAFETY: AppKit is linked above. Each message uses the exact Objective-C
        // ABI for its selector (objects, NSUInteger/usize, pid_t/i32 and BOOL).
        // All returned objects stay alive in this thread's autorelease pool.
        unsafe {
            let _pool = AutoreleasePool(objc_autoreleasePoolPush());
            let string_class = objc_getClass(c"NSString".as_ptr());
            let running_class = objc_getClass(c"NSRunningApplication".as_ptr());
            if string_class.is_null() || running_class.is_null() {
                return false;
            }
            let send_string: unsafe extern "C" fn(Object, Selector, *const c_char) -> Object =
                std::mem::transmute(objc_msgSend as *const ());
            let send_object: unsafe extern "C" fn(Object, Selector, Object) -> Object =
                std::mem::transmute(objc_msgSend as *const ());
            let send_count: unsafe extern "C" fn(Object, Selector) -> usize =
                std::mem::transmute(objc_msgSend as *const ());
            let send_index: unsafe extern "C" fn(Object, Selector, usize) -> Object =
                std::mem::transmute(objc_msgSend as *const ());
            let send_pid: unsafe extern "C" fn(Object, Selector) -> i32 =
                std::mem::transmute(objc_msgSend as *const ());
            let send_activate: unsafe extern "C" fn(Object, Selector, usize) -> ObjcBool =
                std::mem::transmute(objc_msgSend as *const ());

            let identifier = send_string(
                string_class,
                selector(c"stringWithUTF8String:"),
                c"com.nova.flux".as_ptr(),
            );
            let applications = send_object(
                running_class,
                selector(c"runningApplicationsWithBundleIdentifier:"),
                identifier,
            );
            if applications.is_null() {
                return false;
            }
            let count = send_count(applications, selector(c"count"));
            for index in 0..count {
                let application = send_index(applications, selector(c"objectAtIndex:"), index);
                if application.is_null() {
                    continue;
                }
                let pid = send_pid(application, selector(c"processIdentifier"));
                if pid <= 0 || pid as u32 == std::process::id() {
                    continue;
                }
                // NSApplicationActivateAllWindows. This only activates a running
                // app; it never opens a bundle or recursively launches a copy.
                if send_activate(application, selector(c"activateWithOptions:"), 1) as u8 != 0 {
                    return true;
                }
            }
            false
        }
    }

    unsafe fn selector(name: &CStr) -> Selector {
        // SAFETY: Every caller supplies a static, NUL-terminated selector name.
        unsafe { sel_registerName(name.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            loop {
                let path = std::env::temp_dir().join(format!(
                    "nova-flux-single-instance-{}-{}",
                    std::process::id(),
                    NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
                ));
                let directory = fs::DirBuilder::new();
                #[cfg(unix)]
                let directory = {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut directory = directory;
                    directory.mode(0o700);
                    directory
                };
                match directory.create(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("Cannot create private test directory: {error}"),
                }
            }
        }

        fn lock_path(&self) -> PathBuf {
            self.0.join("com.nova.flux/gui.lock")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove private test directory");
        }
    }

    fn guard(acquisition: Acquisition) -> InstanceGuard {
        match acquisition {
            Acquisition::Acquired(guard) => guard,
            Acquisition::AlreadyRunning => panic!("Expected to own an unlocked file"),
        }
    }

    #[test]
    fn independent_handles_cannot_both_own_the_gui_lock() {
        let directory = TestDirectory::new();
        let path = directory.lock_path();
        let _owner = guard(acquire_at(&path).unwrap());
        assert!(matches!(
            acquire_at(&path).unwrap(),
            Acquisition::AlreadyRunning
        ));
        assert!(matches!(
            acquire_at(&path).unwrap(),
            Acquisition::AlreadyRunning
        ));
    }

    #[test]
    fn dropping_owner_allows_reacquiring_the_existing_lock_file() {
        let directory = TestDirectory::new();
        let path = directory.lock_path();
        let owner = guard(acquire_at(&path).unwrap());
        drop(owner);
        assert!(
            path.is_file(),
            "Lock lifetime must never unlink the shared inode"
        );
        let _next_owner = guard(acquire_at(&path).unwrap());
        assert!(matches!(
            acquire_at(&path).unwrap(),
            Acquisition::AlreadyRunning
        ));
    }

    #[test]
    fn stale_file_contents_do_not_claim_ownership_or_get_truncated() {
        let directory = TestDirectory::new();
        let path = directory.lock_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"previous process no longer exists").unwrap();
        let owner = guard(acquire_at(&path).unwrap());
        // Windows locks also deny reads through another handle in this process.
        drop(owner);
        assert_eq!(
            fs::read(&path).unwrap(),
            b"previous process no longer exists"
        );
        let _next_owner = guard(acquire_at(&path).unwrap());
    }

    #[test]
    fn filesystem_errors_cannot_silently_grant_ownership() {
        let directory = TestDirectory::new();
        let parent = directory.0.join("blocked");
        fs::write(&parent, b"not a directory").unwrap();
        assert!(acquire_at(&parent.join("gui.lock")).is_err());
    }
}
