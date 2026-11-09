use std::fs::File;
use std::io;

#[cfg(any(windows, target_vendor = "apple", test))]
const WRITER_LOCK_OFFSET: u64 = i64::MAX as u64 - 1;
#[cfg(any(windows, target_vendor = "apple", test))]
const WRITER_LOCK_LENGTH: u32 = 1;

#[cfg(not(any(windows, target_vendor = "apple")))]
pub(crate) fn try_lock(file: &File) -> io::Result<()> {
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::WouldBlock,
            "另一个 Native owner 持有 writer lock",
        ),
        std::fs::TryLockError::Error(error) => error,
    })
}

#[cfg(target_vendor = "apple")]
pub(crate) fn try_lock(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    // macOS 的 whole-file flock 与 SQLite 的 POSIX byte-range 锁冲突。
    // OFD 锁绑定独立 open 的文件描述，不会被 SQLite 关闭其他 fd 时释放；
    // 锁定文件范围之外的单字节，保留同 inode／hardlink 的单 writer 约束。
    let lock = libc::flock {
        l_start: WRITER_LOCK_OFFSET as libc::off_t,
        l_len: WRITER_LOCK_LENGTH as libc::off_t,
        l_pid: 0,
        l_type: libc::F_WRLCK,
        l_whence: libc::SEEK_SET as libc::c_short,
    };
    // SAFETY: fd 在调用期间有效，lock 指针指向完整初始化的 flock。
    let acquired = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &lock) };
    if acquired == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn try_lock(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::{OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0};

    let mut overlapped = OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 {
            Anonymous: OVERLAPPED_0_0 {
                Offset: WRITER_LOCK_OFFSET as u32,
                OffsetHigh: (WRITER_LOCK_OFFSET >> 32) as u32,
            },
        },
        hEvent: std::ptr::null_mut(),
    };
    let acquired = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            WRITER_LOCK_LENGTH,
            0,
            &mut overlapped,
        )
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
    fn writer_lock_coexists_with_sqlite_and_survives_other_connection_close() {
        let path = std::env::temp_dir().join(format!(
            "native-writer-sqlite-{}-{:?}.sqlite",
            std::process::id(),
            std::thread::current().id()
        ));
        let owner = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        try_lock(&owner).unwrap();
        let contender = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(try_lock(&contender).is_err());
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_millis(1))
            .unwrap();
        connection
            .execute_batch("CREATE TABLE writer_lock_probe (id INTEGER); INSERT INTO writer_lock_probe VALUES (1)")
            .expect("writer lock 必须与 SQLite 自身的文件锁共存");
        drop(connection);
        assert!(
            try_lock(&contender).is_err(),
            "关闭另一个 SQLite fd 不得释放 Native owner 的 writer lock"
        );
        drop(owner);
        try_lock(&contender).unwrap();
        drop(contender);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn writer_byte_is_outside_sqlite_file_and_lock_ranges() {
        let maximum_sqlite_database_bytes = (u32::MAX as u64 - 1) * 65_536;
        let sqlite_shared_lock_end = 0x4000_0000_u64 + 2 + 510;
        assert_eq!(WRITER_LOCK_LENGTH, 1);
        assert!(WRITER_LOCK_OFFSET > maximum_sqlite_database_bytes);
        assert!(WRITER_LOCK_OFFSET > sqlite_shared_lock_end);
        assert_eq!(
            WRITER_LOCK_OFFSET + WRITER_LOCK_LENGTH as u64,
            i64::MAX as u64
        );
    }
}
