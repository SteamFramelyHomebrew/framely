use anyhow::{Result, ensure};
use if_addrs::IfAddr;
use mdns_sd::{HostnameResolutionEvent, ServiceDaemon, ServiceEvent};
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Device {
    pub ip: String,
    pub name: String,
    pub ssh: bool,
}
pub fn is_frame_name(name: &str) -> bool {
    name.trim_end_matches('.')
        .split('.')
        .next()
        .is_some_and(|host| host.eq_ignore_ascii_case("frame"))
}
pub fn networks() -> Vec<String> {
    let mut result = BTreeSet::new();
    for interface in if_addrs::get_if_addrs().unwrap_or_default() {
        if let IfAddr::V4(v4) = interface.addr {
            if v4.ip.is_loopback() || v4.ip.is_link_local() {
                continue;
            }
            let mask = u32::from(v4.netmask);
            let prefix = mask.count_ones().max(22); // Avoid enormous corporate networks by default.
            let mask = u32::MAX << (32 - prefix);
            result.insert(format!(
                "{}/{}",
                Ipv4Addr::from(u32::from(v4.ip) & mask),
                prefix
            ));
        }
    }
    let mut networks: Vec<_> = result.into_iter().collect();
    networks.sort_by_key(|network| {
        let priority = if network.starts_with("192.168.") {
            0
        } else if network.starts_with("192.") {
            1
        } else {
            2
        };
        (priority, network.clone())
    });
    networks
}
pub fn addresses(cidr: &str) -> Result<Vec<Ipv4Addr>> {
    let (ip, prefix) = cidr
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("网段格式应为 192.168.1.0/24"))?;
    let ip: Ipv4Addr = ip.parse()?;
    let prefix: u32 = prefix.parse()?;
    ensure!(
        (22..=32).contains(&prefix),
        "每次扫描最多 1024 个地址，请使用 /22 到 /32 网段"
    );
    let mask = if prefix == 32 {
        u32::MAX
    } else {
        u32::MAX << (32 - prefix)
    };
    let base = u32::from(ip) & mask;
    let size = 1u32 << (32 - prefix);
    let range = if size > 2 { 1..size - 1 } else { 0..size };
    Ok(range.map(|offset| Ipv4Addr::from(base + offset)).collect())
}
fn ssh(ip: IpAddr) -> bool {
    let Ok(stream) =
        TcpStream::connect_timeout(&SocketAddr::new(ip, 22), Duration::from_millis(450))
    else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(700)));
    let mut reader = BufReader::new(stream);
    let mut total = 0;
    for _ in 0..10 {
        let mut line = String::new();
        let Ok(n) = reader.by_ref().take(1024).read_line(&mut line) else {
            return false;
        };
        total += n;
        if n == 0 || total > 4096 {
            return false;
        }
        if line.starts_with("SSH-2.0-") {
            return true;
        }
    }
    false
}
pub fn scan(
    cidr: &str,
    cancelled: Arc<AtomicBool>,
    emit: Arc<dyn Fn(Device) + Send + Sync>,
) -> Result<()> {
    let ips = Arc::new(addresses(cidr)?);
    let index = Arc::new(AtomicUsize::new(0));
    // Name discovery starts simultaneously; it never gates IP probing.
    let names_emit = emit.clone();
    let name_cancel = cancelled.clone();
    thread::spawn(move || {
        for name in ["frame.local:22", "frame:22"] {
            if name_cancel.load(Ordering::Relaxed) {
                return;
            }
            if let Ok(addresses) = name.to_socket_addrs() {
                for addr in addresses.filter(|a| a.is_ipv4()) {
                    names_emit(Device {
                        ip: addr.ip().to_string(),
                        name: "frame".into(),
                        ssh: ssh(addr.ip()),
                    });
                }
            }
        }
    });
    let mdns_emit = emit.clone();
    let mdns_cancel = cancelled.clone();
    let mdns_thread = thread::spawn(move || {
        let Ok(daemon) = ServiceDaemon::new() else {
            return;
        };
        // A host can publish frame.local without advertising an SSH service.
        let hostname_events = daemon.resolve_hostname("frame.local.", Some(5000)).ok();
        let service_events = daemon.browse("_ssh._tcp.local.").ok();
        let mut found = BTreeSet::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) && !mdns_cancel.load(Ordering::Relaxed) {
            let mut addresses = Vec::new();
            if let Some(events) = &hostname_events
                && let Ok(HostnameResolutionEvent::AddressesFound(name, ips)) =
                    events.recv_timeout(Duration::from_millis(100))
                && is_frame_name(&name)
            {
                addresses.extend(ips);
            }
            if let Some(events) = &service_events {
                while let Ok(event) = events.try_recv() {
                    if let ServiceEvent::ServiceResolved(info) = event
                        && is_frame_name(info.get_hostname())
                    {
                        addresses.extend(info.get_addresses());
                    }
                }
            }
            for ip in addresses.into_iter().filter(|ip| ip.is_ipv4()) {
                if found.insert(ip) && !mdns_cancel.load(Ordering::Relaxed) {
                    mdns_emit(Device {
                        ip: ip.to_string(),
                        name: "frame".into(),
                        ssh: ssh(ip),
                    });
                }
            }
            if hostname_events.is_none() {
                thread::sleep(Duration::from_millis(100));
            }
        }
        let _ = daemon.shutdown();
    });
    thread::scope(|scope| {
        for _ in 0..32 {
            let ips = ips.clone();
            let index = index.clone();
            let emit = emit.clone();
            let cancelled = cancelled.clone();
            scope.spawn(move || {
                loop {
                    if cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let i = index.fetch_add(1, Ordering::Relaxed);
                    let Some(ip) = ips.get(i) else { break };
                    if ssh(IpAddr::V4(*ip)) {
                        let ip = *ip;
                        let emit = emit.clone();
                        let cancelled = cancelled.clone();
                        thread::spawn(move || {
                            if let Ok(name) = dns_lookup::lookup_addr(&IpAddr::V4(ip))
                                && is_frame_name(&name)
                                && !cancelled.load(Ordering::Relaxed)
                            {
                                emit(Device {
                                    ip: ip.to_string(),
                                    name: "frame".into(),
                                    ssh: true,
                                });
                            }
                        });
                    }
                }
            });
        }
    });
    let _ = mdns_thread.join();
    Ok(())
}
use std::io::Read;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_name_requires_exact_hostname() {
        for name in ["frame", "FRAME", "frame.local.", "frame.lan"] {
            assert!(is_frame_name(name), "{name}");
        }
        for name in [
            "framework",
            "frame-living-room",
            "frame-2.local",
            "pc.local",
            "SSH 设备（名称待确认）",
            "",
        ] {
            assert!(!is_frame_name(name), "{name}");
        }
    }
    #[test]
    fn subnet_bounds() {
        assert_eq!(addresses("192.168.2.90/24").unwrap().len(), 254);
        assert_eq!(
            addresses("192.168.2.90/32").unwrap(),
            vec![Ipv4Addr::new(192, 168, 2, 90)]
        );
        assert!(addresses("10.0.0.0/8").is_err());
    }
}
