//! Local network interface queries.

use std::net::Ipv4Addr;

/// Broadcast addresses of the IPv4 interfaces that are up, excluding loopback.
#[cfg(unix)]
pub(crate) fn ipv4_broadcast_addrs() -> std::io::Result<Vec<Ipv4Addr>> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `head` with a list that is freed below.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut addrs = Vec::new();
    let mut cur = head;
    while !cur.is_null() {
        // SAFETY: `cur` is a node of the list returned by getifaddrs.
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        let flags = ifa.ifa_flags as libc::c_int;
        let wanted = libc::IFF_UP | libc::IFF_BROADCAST;
        if flags & wanted != wanted || flags & libc::IFF_LOOPBACK != 0 {
            continue;
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let broadcast = ifa.ifa_ifu;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let broadcast = ifa.ifa_dstaddr;
        if broadcast.is_null() {
            continue;
        }
        // SAFETY: non-null sockaddr from getifaddrs; the family is checked
        // before reading it as sockaddr_in.
        unsafe {
            if (*broadcast).sa_family as libc::c_int == libc::AF_INET {
                let sin = &*(broadcast as *const libc::sockaddr_in);
                addrs.push(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
            }
        }
    }
    // SAFETY: `head` came from getifaddrs and is freed once.
    unsafe { libc::freeifaddrs(head) };
    addrs.sort();
    addrs.dedup();
    Ok(addrs)
}

/// Without getifaddrs, fall back to the limited broadcast address.
#[cfg(not(unix))]
pub(crate) fn ipv4_broadcast_addrs() -> std::io::Result<Vec<Ipv4Addr>> {
    Ok(vec![Ipv4Addr::BROADCAST])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_broadcast_addresses() {
        let addrs = ipv4_broadcast_addrs().unwrap();
        assert!(addrs.iter().all(|a| !a.is_loopback()));
    }
}
