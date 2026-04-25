use process_memory::{TryIntoProcessHandle, copy_address};

/// Process memory reader using /proc/[pid]/mem for zero-copy reads on Linux.
/// This is more efficient than process_vm_readv for multiple reads as we keep the file open.
#[cfg(target_os = "linux")]
pub struct ProcessMemReader {
    file: std::fs::File,
}

#[cfg(target_os = "linux")]
impl ProcessMemReader {
    pub fn new(pid: process_memory::Pid) -> std::io::Result<Self> {
        use std::fs::OpenOptions;
        let path = format!("/proc/{pid}/mem");
        let file = OpenOptions::new().read(true).open(&path)?;
        Ok(Self { file })
    }

    /// Read memory at the given address using pread (no seeking required, thread-safe)
    pub fn read_at(&self, address: usize, size: usize) -> std::io::Result<Vec<u8>> {
        use std::io::Error;
        use std::os::unix::io::AsRawFd;

        let mut buffer = vec![0u8; size];
        let fd = self.file.as_raw_fd();

        // SAFETY: `fd` is a valid file descriptor owned by `self.file` for the
        // duration of this call (the &self borrow keeps the file alive).
        // `buffer` is a freshly allocated `Vec<u8>` of length `size`, so the
        // pointer is valid for `size` bytes of writes. `pread` does not move
        // the file offset, making concurrent reads from multiple threads safe.
        let result = unsafe { libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, size, address as libc::off_t) };

        if result == -1 {
            Err(Error::last_os_error())
        } else if (result as usize) < size {
            buffer.truncate(result as usize);
            Ok(buffer)
        } else {
            Ok(buffer)
        }
    }

    /// Read directly into an existing buffer (avoids allocation)
    pub fn read_into(&self, address: usize, buffer: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Error;
        use std::os::unix::io::AsRawFd;

        let fd = self.file.as_raw_fd();

        // SAFETY: `fd` is a valid file descriptor for the lifetime of `&self`.
        // `buffer` is a unique `&mut [u8]`, so writing `buffer.len()` bytes
        // through `buffer.as_mut_ptr()` is sound. `pread` is offset-stable
        // and may be called concurrently on the same fd from multiple threads.
        let result = unsafe { libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len(), address as libc::off_t) };

        if result == -1 { Err(Error::last_os_error()) } else { Ok(result as usize) }
    }
}

/// Fast memory read using process_vm_readv on Linux (fastest method).
/// Falls back to /proc/[pid]/mem + pread on error.
#[cfg(target_os = "linux")]
pub(super) fn fast_read_memory(pid: process_memory::Pid, address: usize, size: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::fs::OpenOptions;
    use std::io::Error;
    use std::os::unix::io::AsRawFd;

    // Try process_vm_readv first (fastest based on benchmarks: ~670 MB/s vs ~540 MB/s)
    let mut buffer = vec![0u8; size];

    let local_iov = libc::iovec {
        iov_base: buffer.as_mut_ptr() as *mut libc::c_void,
        iov_len: size,
    };

    let remote_iov = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: size,
    };

    // SAFETY: Both iovecs describe valid memory:
    //  * `local_iov` points into `buffer`, which is alive for this call and
    //    sized to `size` bytes.
    //  * `remote_iov` describes addresses in the target process; the kernel
    //    validates them and returns EFAULT/partial-read on bad pages instead
    //    of dereferencing them in our address space.
    // `process_vm_readv` is documented as thread-safe and does not retain
    // pointers past the call.
    let result = unsafe { libc::process_vm_readv(pid as libc::pid_t, &local_iov as *const libc::iovec, 1, &remote_iov as *const libc::iovec, 1, 0) };

    if result > 0 {
        if (result as usize) < size {
            buffer.truncate(result as usize);
        }
        return Ok(buffer);
    }

    // Fallback to /proc/[pid]/mem with pread
    let path = format!("/proc/{pid}/mem");
    if let Ok(file) = OpenOptions::new().read(true).open(&path) {
        let mut buffer = vec![0u8; size];
        let fd = file.as_raw_fd();

        // SAFETY: `fd` is a valid file descriptor owned by `file`, which is
        // kept alive on the next line by the surrounding scope. `buffer` is
        // freshly allocated and exclusively owned, so writing `size` bytes
        // into it is sound. `pread` does not move the file offset.
        let result = unsafe { libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, size, address as libc::off_t) };

        if result > 0 {
            if (result as usize) < size {
                buffer.truncate(result as usize);
            }
            return Ok(buffer);
        }
    }

    // Final fallback to copy_address
    match pid.try_into_process_handle() {
        Ok(handle) => copy_address(address, size, &handle).map_err(|e| Error::other(e.to_string())),
        Err(e) => Err(Error::other(e.to_string())),
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn fast_read_memory(pid: process_memory::Pid, address: usize, size: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::io::{Error, ErrorKind};

    match pid.try_into_process_handle() {
        Ok(handle) => copy_address(address, size, &handle).map_err(|e| Error::new(ErrorKind::Other, e.to_string())),
        Err(e) => Err(Error::new(ErrorKind::Other, e.to_string())),
    }
}
