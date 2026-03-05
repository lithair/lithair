//! Process RSS from `/proc/self/status`.

/// Parse `VmRSS` from `/proc/self/status` content.
///
/// Looks for `VmRSS:    <value> kB` and returns bytes.
pub fn parse_vm_rss(content: &str) -> Option<u64> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

/// Read the current process RSS from `/proc/self/status`.
pub fn read_process_rss() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_vm_rss(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Name:\tmy_app
Umask:\t0022
State:\tS (sleeping)
Tgid:\t12345
Pid:\t12345
VmPeak:\t  524288 kB
VmSize:\t  491520 kB
VmRSS:\t  153600 kB
VmData:\t  262144 kB
";

    #[test]
    fn parse_rss() {
        assert_eq!(parse_vm_rss(SAMPLE), Some(153_600 * 1024));
    }

    #[test]
    fn no_vmrss_line() {
        let content = "Name:\tmy_app\nState:\tS\n";
        assert!(parse_vm_rss(content).is_none());
    }
}
