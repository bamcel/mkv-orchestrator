//! Reaching network locations that no drive letter points at.
//!
//! A UNC share is an ordinary directory once you name it in full, so listing
//! `\\server\share\...` needs nothing special. A server on its own is the
//! exception: `read_dir(r"\\server")` fails with "the network name cannot be
//! found" because a bare server is not a directory at all. Its shares have to
//! be enumerated through `NetShareEnum`, which is what this module is for.

/// The two halves of UNC that behave differently, so callers can branch once
/// rather than guessing from an opaque IO error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UncTarget {
    /// `\\server` — listable only by enumerating its shares.
    Server(String),
    /// `\\server\share` or deeper — an ordinary directory. The server is
    /// carried along because Rust folds `\\server\share` into a single path
    /// prefix, so a share root reports no parent and the caller has to
    /// reconstruct the server to offer "up".
    Share { server: String },
}

/// Classifies a path as a network location, or `None` when it is a local path.
///
/// The `\\?\` and `\\.\` prefixes also begin with two separators but address
/// devices rather than servers, so they are deliberately not network paths.
pub fn classify_unc(path: &str) -> Option<UncTarget> {
    let trimmed = path.trim();
    let rest = trimmed
        .strip_prefix(r"\\")
        .or_else(|| trimmed.strip_prefix("//"))?;

    let mut segments = rest
        .split(['\\', '/'])
        .filter(|segment| !segment.is_empty());

    let server = segments.next()?;
    if server == "?" || server == "." {
        return None;
    }

    match segments.next() {
        Some(_) => Some(UncTarget::Share {
            server: server.to_owned(),
        }),
        None => Some(UncTarget::Server(server.to_owned())),
    }
}

/// A share as the browser wants to show it: the name on its own, and the full
/// path to navigate to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    pub name: String,
    pub path: String,
}

#[cfg(windows)]
mod windows_shares {
    use super::Share;
    use std::ffi::c_void;
    use std::io;

    // Declared here rather than taken from `windows-sys`, which does not
    // expose the share-enumeration API.
    #[repr(C)]
    struct ShareInfo1 {
        netname: *mut u16,
        share_type: u32,
        remark: *mut u16,
    }

    #[link(name = "netapi32")]
    unsafe extern "system" {
        fn NetShareEnum(
            servername: *const u16,
            level: u32,
            bufptr: *mut *mut u8,
            prefmaxlen: u32,
            entriesread: *mut u32,
            totalentries: *mut u32,
            resume_handle: *mut u32,
        ) -> u32;
        fn NetApiBufferFree(buffer: *mut c_void) -> u32;
    }

    const MAX_PREFERRED_LENGTH: u32 = u32::MAX;
    const NERR_SUCCESS: u32 = 0;
    const ERROR_MORE_DATA: u32 = 234;
    /// The low byte carries the kind; the high bits are flags.
    const STYPE_MASK: u32 = 0xFF;
    const STYPE_DISKTREE: u32 = 0;
    /// Administrative shares such as `C$` and `IPC$`, which Explorer hides.
    const STYPE_SPECIAL: u32 = 0x8000_0000;

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reads a NUL-terminated UTF-16 string that the API allocated.
    ///
    /// # Safety
    /// `pointer` must be NUL-terminated and valid for reads.
    unsafe fn from_wide(pointer: *const u16) -> String {
        if pointer.is_null() {
            return String::new();
        }
        let mut length = 0usize;
        // SAFETY: the caller guarantees a NUL-terminated buffer.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: `length` stops at the terminator, so the range is in bounds.
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
    }

    pub fn list(server: &str) -> io::Result<Vec<Share>> {
        let name = to_wide(&format!(r"\\{server}"));
        let mut buffer: *mut u8 = std::ptr::null_mut();
        let mut read = 0u32;
        let mut total = 0u32;
        let mut resume = 0u32;

        // SAFETY: `name` is NUL-terminated and outlives the call; the out
        // params are all valid for writes.
        let status = unsafe {
            NetShareEnum(
                name.as_ptr(),
                1,
                &raw mut buffer,
                MAX_PREFERRED_LENGTH,
                &raw mut read,
                &raw mut total,
                &raw mut resume,
            )
        };

        // MAX_PREFERRED_LENGTH asks for one buffer big enough for everything,
        // so a partial read means the server truncated it rather than that we
        // must resume.
        if status != NERR_SUCCESS && status != ERROR_MORE_DATA {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        if buffer.is_null() {
            return Ok(Vec::new());
        }

        let mut shares = Vec::new();
        for index in 0..read as usize {
            // SAFETY: the API reports `read` valid entries at `buffer`.
            let entry = unsafe { &*buffer.cast::<ShareInfo1>().add(index) };
            if entry.share_type & STYPE_MASK != STYPE_DISKTREE
                || entry.share_type & STYPE_SPECIAL != 0
            {
                continue;
            }
            // SAFETY: the API returns NUL-terminated names.
            let netname = unsafe { from_wide(entry.netname) };
            if netname.is_empty() {
                continue;
            }
            shares.push(Share {
                path: format!(r"\\{server}\{netname}"),
                name: netname,
            });
        }

        // SAFETY: `buffer` came from NetShareEnum and is freed exactly once.
        unsafe { NetApiBufferFree(buffer.cast::<c_void>()) };

        shares.sort_by_key(|share| share.name.to_lowercase());
        Ok(shares)
    }
}

/// Lists the disk shares a server publishes, hiding administrative ones.
pub fn list_server_shares(server: &str) -> std::io::Result<Vec<Share>> {
    #[cfg(windows)]
    {
        windows_shares::list(server)
    }
    #[cfg(not(windows))]
    {
        let _ = server;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "listing the shares of a server is only supported on Windows; \
             mount the share and browse the mount point instead",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_server_is_distinguished_from_a_share() {
        assert_eq!(
            classify_unc(r"\\192.168.1.100"),
            Some(UncTarget::Server("192.168.1.100".to_owned()))
        );
        assert_eq!(
            classify_unc(r"\\192.168.1.100\"),
            Some(UncTarget::Server("192.168.1.100".to_owned()))
        );
        let share = UncTarget::Share {
            server: "192.168.1.100".to_owned(),
        };
        assert_eq!(
            classify_unc(r"\\192.168.1.100\downloads"),
            Some(share.clone())
        );
        assert_eq!(
            classify_unc(r"\\192.168.1.100\downloads\completed"),
            Some(share)
        );
    }

    /// Users paste paths from browsers and config files, so forward slashes
    /// have to mean the same thing.
    #[test]
    fn forward_slashes_are_accepted() {
        assert_eq!(
            classify_unc("//nas/media"),
            Some(UncTarget::Share {
                server: "nas".to_owned()
            })
        );
        assert_eq!(
            classify_unc("//nas"),
            Some(UncTarget::Server("nas".to_owned()))
        );
    }

    /// `\\?\` opens a *local* path in verbatim form. Treating it as a server
    /// would send `C:` to the share enumerator.
    #[test]
    fn device_prefixes_are_not_network_paths() {
        assert_eq!(classify_unc(r"\\?\C:\media"), None);
        assert_eq!(classify_unc(r"\\.\PhysicalDrive0"), None);
    }

    #[test]
    fn local_paths_are_not_network_paths() {
        assert_eq!(classify_unc(r"C:\media"), None);
        assert_eq!(classify_unc("/mnt/media"), None);
        assert_eq!(classify_unc(""), None);
    }
}
