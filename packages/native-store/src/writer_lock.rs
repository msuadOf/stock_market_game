use std::fs::File;
use std::io;

#[cfg(any(windows, test))]
const WINDOWS_WRITER_LOCK_OFFSET: u64 = i64::MAX as u64 - 1;
#[cfg(any(windows, test))]
const WINDOWS_WRITER_LOCK_LENGTH: u32 = 1;

#[cfg(not(windows))]
pub(crate) fn try_lock(file: &File) -> io::Result<()> {
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => io::Error::new(io::ErrorKind::WouldBlock, "另一个 Native owner 持有 writer lock"),
        std::fs::TryLockError::Error(error) => error,
    })
}

#[cfg(windows)]
pub(crate) fn try_lock(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};
    use windows_sys::Win32::System::IO::{OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0};

    let mut overlapped = OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 { Anonymous: OVERLAPPED_0_0 {
            Offset: WINDOWS_WRITER_LOCK_OFFSET as u32,
            OffsetHigh: (WINDOWS_WRITER_LOCK_OFFSET >> 32) as u32,
        } },
        hEvent: std::ptr::null_mut(),
    };
    let acquired = unsafe {
        LockFileEx(file.as_raw_handle(), LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, WINDOWS_WRITER_LOCK_LENGTH, 0, &mut overlapped)
    };
    if acquired == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_writer_byte_is_outside_sqlite_file_and_lock_ranges() {
        let maximum_sqlite_database_bytes = (u32::MAX as u64 - 1) * 65_536;
        let sqlite_shared_lock_end = 0x4000_0000_u64 + 2 + 510;
        assert_eq!(WINDOWS_WRITER_LOCK_LENGTH, 1);
        assert!(WINDOWS_WRITER_LOCK_OFFSET > maximum_sqlite_database_bytes);
        assert!(WINDOWS_WRITER_LOCK_OFFSET > sqlite_shared_lock_end);
        assert_eq!(WINDOWS_WRITER_LOCK_OFFSET + WINDOWS_WRITER_LOCK_LENGTH as u64, i64::MAX as u64);
    }
}
