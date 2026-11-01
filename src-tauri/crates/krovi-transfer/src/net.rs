//! Finding the one network interface Quick Transfer is allowed to listen on.
//!
//! Rules (all in `pick`, pure and unit-tested):
//!   * the adapter must be up and carry a private IPv4 address (10/8, 172.16/12, 192.168/16);
//!   * loopback, tunnels, VPN/virtual/bridge adapters (Hyper-V, WSL, VirtualBox, VMware, Docker,
//!     TAP, Tailscale, ZeroTier...) are never used;
//!   * adapters with a default gateway win (that is "the Wi-Fi/Ethernet the internet comes in
//!     on"); among them the one the OS would route a packet through, then the lowest metric;
//!   * the Windows Mobile Hotspot adapter is used only when nothing better exists, so a phone
//!     joined to this PC's own hotspot still works.
//!
//! Enumeration uses `GetAdaptersAddresses` on Windows. The only "probe" is a UDP `connect()`
//! to a documentation-range address, which asks the OS routing table which local address it
//! would use; connecting a UDP socket sends no packet.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    pub description: String,
    pub ip: Ipv4Addr,
    pub up: bool,
    pub has_gateway: bool,
    pub metric: u32,
    pub if_type: u32,
}

pub const IF_TYPE_PPP: u32 = 23;
pub const IF_TYPE_LOOPBACK: u32 = 24;
pub const IF_TYPE_TUNNEL: u32 = 131;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Real adapter with a default gateway.
    Gateway = 0,
    /// Real adapter without a gateway (isolated LAN, direct cable).
    NoGateway = 1,
    /// Windows Mobile Hotspot / Wi-Fi Direct virtual adapter.
    Hotspot = 2,
}

const VIRTUAL_MARKERS: [&str; 24] = [
    "vethernet",
    "hyper-v",
    "virtualbox",
    "vmware",
    "vmnet",
    "wsl",
    "docker",
    "tap-windows",
    "tap-",
    "npcap",
    "loopback",
    "tailscale",
    "zerotier",
    "wireguard",
    "openvpn",
    "nordlynx",
    "bluetooth",
    "teredo",
    "isatap",
    "vpn",
    "hamachi",
    "radmin",
    "warp",
    "pseudo-interface",
];

/// `None` means "never use this adapter".
pub fn classify(a: &Adapter) -> Option<Tier> {
    if !a.up || !is_private_v4(a.ip) {
        return None;
    }
    if matches!(a.if_type, IF_TYPE_LOOPBACK | IF_TYPE_TUNNEL | IF_TYPE_PPP) {
        return None;
    }
    let text = format!("{} {}", a.name, a.description).to_ascii_lowercase();
    if text.contains("wi-fi direct")
        || text.contains("hosted network")
        || text.contains("virtual wifi")
    {
        return Some(Tier::Hotspot);
    }
    if VIRTUAL_MARKERS.iter().any(|m| text.contains(m)) {
        return None;
    }
    Some(if a.has_gateway {
        Tier::Gateway
    } else {
        Tier::NoGateway
    })
}

pub fn is_private_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() && !ip.is_loopback() && !ip.is_link_local()
}

/// Choose the interface. `route_src` is the local address the OS would use for outbound
/// traffic (see `route_source`); it breaks ties between several real adapters.
pub fn pick(adapters: &[Adapter], route_src: Option<Ipv4Addr>) -> Option<Adapter> {
    let mut c: Vec<(Tier, bool, u32, &Adapter)> = adapters
        .iter()
        .filter_map(|a| classify(a).map(|t| (t, Some(a.ip) != route_src, a.metric, a)))
        .collect();
    c.sort_by_key(|(t, not_route, metric, _)| (*t, *not_route, *metric));
    c.first().map(|(_, _, _, a)| (*a).clone())
}

/// The local address the OS would pick for traffic leaving this machine. Sends nothing.
pub fn route_source() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    // 192.0.2.0/24 is TEST-NET-1 (RFC 5737): reserved for documentation, never routed.
    s.connect("192.0.2.1:9").ok()?;
    match s.local_addr().ok()? {
        SocketAddr::V4(a) => Some(*a.ip()),
        SocketAddr::V6(_) => None,
    }
}

/// The detected interface Quick Transfer will bind to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lan {
    pub ip: Ipv4Addr,
    pub adapter: String,
}

pub fn detect() -> Option<Lan> {
    let adapters = enumerate();
    pick(&adapters, route_source()).map(|a| Lan {
        ip: a.ip,
        adapter: a.name,
    })
}

#[cfg(windows)]
pub fn enumerate() -> Vec<Adapter> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };

    const AF_INET: u32 = 2;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    const IF_OPER_STATUS_UP: i32 = 1;

    let flags = GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_INCLUDE_GATEWAYS;
    let mut size: u32 = 16 * 1024;
    let mut out = Vec::new();
    for _ in 0..4 {
        // u64 elements guarantee the 8-byte alignment IP_ADAPTER_ADDRESSES requires.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let head = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        // SAFETY: `head` points to a zeroed, 8-byte-aligned buffer of at least `size` bytes;
        // GetAdaptersAddresses fills it and updates `size` when it is too small.
        let ret =
            unsafe { GetAdaptersAddresses(AF_INET, flags, std::ptr::null(), head, &mut size) };
        if ret == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if ret != 0 {
            return out;
        }
        let mut cur = head as *const IP_ADAPTER_ADDRESSES_LH;
        while !cur.is_null() {
            // SAFETY: `cur` is a node of the linked list GetAdaptersAddresses wrote into `buf`,
            // which outlives this loop; every pointer we follow is checked for null first.
            let a = unsafe { &*cur };
            let name = unsafe { wide(a.FriendlyName) };
            let description = unsafe { wide(a.Description) };
            let has_gateway = !a.FirstGatewayAddress.is_null();
            let mut u = a.FirstUnicastAddress as *const windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_UNICAST_ADDRESS_LH;
            while !u.is_null() {
                let ua = unsafe { &*u };
                let sa = ua.Address.lpSockaddr as *const u8;
                if !sa.is_null() && ua.Address.iSockaddrLength >= 8 {
                    // sockaddr_in layout: family:u16 (LE on Windows), port:u16, addr:[u8;4]
                    let (fam, ip) = unsafe {
                        (
                            u16::from_le_bytes([*sa, *sa.add(1)]),
                            [*sa.add(4), *sa.add(5), *sa.add(6), *sa.add(7)],
                        )
                    };
                    if u32::from(fam) == AF_INET {
                        out.push(Adapter {
                            name: name.clone(),
                            description: description.clone(),
                            ip: Ipv4Addr::from(ip),
                            up: a.OperStatus == IF_OPER_STATUS_UP,
                            has_gateway,
                            metric: a.Ipv4Metric,
                            if_type: a.IfType,
                        });
                    }
                }
                u = ua.Next as *const _;
            }
            cur = a.Next as *const _;
        }
        return out;
    }
    out
}

#[cfg(windows)]
unsafe fn wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 && len < 512 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

#[cfg(not(windows))]
pub fn enumerate() -> Vec<Adapter> {
    // Non-Windows hosts (CI, Linux dev boxes) have no adapter list here; fall back to the
    // routing table's answer so tests and examples still find an address.
    match route_source() {
        Some(ip) if is_private_v4(ip) => vec![Adapter {
            name: "default route".into(),
            description: String::new(),
            ip,
            up: true,
            has_gateway: true,
            metric: 0,
            if_type: 6,
        }],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ad(name: &str, desc: &str, ip: [u8; 4], gw: bool, metric: u32) -> Adapter {
        Adapter {
            name: name.into(),
            description: desc.into(),
            ip: Ipv4Addr::from(ip),
            up: true,
            has_gateway: gw,
            metric,
            if_type: 71,
        }
    }

    #[test]
    fn private_ranges() {
        for ip in ["10.0.0.5", "172.16.0.1", "172.31.255.254", "192.168.1.9"] {
            assert!(is_private_v4(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "172.32.0.1",
            "127.0.0.1",
            "169.254.10.10",
            "100.64.1.1",
            "0.0.0.0",
            "224.0.0.1",
        ] {
            assert!(!is_private_v4(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn picks_the_wifi_with_a_gateway_over_virtual_adapters() {
        let list = vec![
            ad(
                "vEthernet (WSL)",
                "Hyper-V Virtual Ethernet Adapter",
                [172, 20, 80, 1],
                false,
                5000,
            ),
            ad(
                "VirtualBox Host-Only Network",
                "VirtualBox Host-Only Ethernet Adapter",
                [192, 168, 56, 1],
                false,
                25,
            ),
            ad(
                "Wi-Fi",
                "Intel(R) Wi-Fi 6 AX201 160MHz",
                [192, 168, 1, 42],
                true,
                40,
            ),
        ];
        let p = pick(&list, None).unwrap();
        assert_eq!(p.ip, Ipv4Addr::new(192, 168, 1, 42));
    }

    #[test]
    fn never_picks_vpn_tunnel_loopback_or_public_addresses() {
        let mut tun = ad("Tunnel", "Teredo Tunneling", [10, 1, 1, 1], true, 1);
        tun.if_type = IF_TYPE_TUNNEL;
        let list = vec![
            tun,
            ad("Tailscale", "Tailscale Tunnel", [10, 9, 9, 9], true, 1),
            ad("Ethernet", "Realtek PCIe GbE", [8, 8, 4, 4], true, 1), // public address
            ad("Loopback", "Software Loopback", [127, 0, 0, 1], false, 1),
        ];
        assert_eq!(pick(&list, None), None);
    }

    #[test]
    fn down_adapters_are_ignored() {
        let mut w = ad("Wi-Fi", "Intel Wi-Fi", [192, 168, 1, 2], true, 10);
        w.up = false;
        assert_eq!(pick(&[w], None), None);
    }

    #[test]
    fn route_source_breaks_ties_between_real_adapters() {
        let list = vec![
            ad("Ethernet", "Realtek", [192, 168, 0, 7], true, 25),
            ad("Wi-Fi", "Intel Wi-Fi", [192, 168, 1, 42], true, 40),
        ];
        // without a hint the lower metric wins
        assert_eq!(pick(&list, None).unwrap().ip, Ipv4Addr::new(192, 168, 0, 7));
        // the interface the OS routes through wins over metric
        assert_eq!(
            pick(&list, Some(Ipv4Addr::new(192, 168, 1, 42)))
                .unwrap()
                .ip,
            Ipv4Addr::new(192, 168, 1, 42)
        );
    }

    #[test]
    fn gateway_beats_no_gateway_and_hotspot_is_last_resort() {
        let hotspot = ad(
            "Local Area Connection* 10",
            "Microsoft Wi-Fi Direct Virtual Adapter #2",
            [192, 168, 137, 1],
            false,
            10,
        );
        let lan_nogw = ad("Ethernet 2", "USB GbE", [10, 0, 0, 5], false, 10);
        let wifi = ad("Wi-Fi", "Intel Wi-Fi", [192, 168, 1, 42], true, 600);
        assert_eq!(
            pick(&[hotspot.clone(), lan_nogw.clone(), wifi.clone()], None)
                .unwrap()
                .ip,
            wifi.ip
        );
        assert_eq!(
            pick(&[hotspot.clone(), lan_nogw.clone()], None).unwrap().ip,
            lan_nogw.ip
        );
        assert_eq!(
            pick(std::slice::from_ref(&hotspot), None).unwrap().ip,
            hotspot.ip
        );
        assert_eq!(classify(&hotspot), Some(Tier::Hotspot));
    }

    #[test]
    fn route_source_probe_never_panics() {
        // On a machine with no default route this is simply None.
        let _ = route_source();
    }
}
