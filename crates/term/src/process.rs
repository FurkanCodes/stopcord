//! Looking up a process by id: its name and its arguments.

/// A running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    /// The executable's short name, such as `node` or `claude`.
    pub name: String,
    /// Its arguments, starting with argv[0]. Empty when they can't be read.
    pub argv: Vec<String>,
}

pub fn process_info(pid: u32) -> Option<Process> {
    let name = name(pid)?;
    Some(Process {
        pid,
        name,
        argv: argv(pid).unwrap_or_default(),
    })
}

#[cfg(target_os = "macos")]
fn name(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; 256];
    // SAFETY: the buffer is valid for its full length.
    let n = unsafe {
        libc::proc_name(
            pid as libc::c_int,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
        )
    };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    String::from_utf8(buf).ok()
}

#[cfg(target_os = "macos")]
fn argv(pid: u32) -> Option<Vec<String>> {
    // KERN_PROCARGS2 gives: argc (i32), the executable path, padding NULs,
    // then argc NUL-terminated arguments, then the environment.
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;
    // SAFETY: a null buffer asks only for the size.
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: the buffer is `size` bytes long, as sysctl asked.
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 {
        return None;
    }
    buf.truncate(size);
    parse_procargs(&buf)
}

#[cfg(target_os = "macos")]
fn parse_procargs(buf: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?) as usize;
    let mut rest = &buf[4..];
    // Skip the executable path and the NULs after it.
    let path_end = rest.iter().position(|&b| b == 0)?;
    rest = &rest[path_end..];
    let start = rest.iter().position(|&b| b != 0)?;
    rest = &rest[start..];
    let args = rest
        .split(|&b| b == 0)
        .take(argc)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect::<Vec<_>>();
    Some(args)
}

#[cfg(target_os = "linux")]
fn name(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(comm.trim_end().to_string())
}

#[cfg(target_os = "linux")]
fn argv(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        raw.split(|&b| b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect(),
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn name(_pid: u32) -> Option<String> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn argv(_pid: u32) -> Option<Vec<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_process() {
        let me = process_info(std::process::id()).expect("can't read this process");
        assert!(!me.name.is_empty());
        assert!(!me.argv.is_empty());
    }
}
