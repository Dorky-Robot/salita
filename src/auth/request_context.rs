use axum::extract::ConnectInfo;
use std::net::{IpAddr, SocketAddr};

/// Represents the origin of a request based on network location
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOrigin {
    /// Request from localhost (127.0.0.1, ::1)
    Localhost,
    /// Request from LAN (10.x, 172.16-31.x, 192.168.x)
    Lan,
    /// Request from external network (ngrok, public domains)
    External,
}

/// Check if a socket address is from localhost
pub fn is_local_request(connect_info: &ConnectInfo<SocketAddr>) -> bool {
    let ip = connect_info.0.ip();

    match ip {
        // IPv4 localhost
        IpAddr::V4(addr) => addr.is_loopback(),
        // IPv6 localhost (::1) or IPv6-mapped IPv4 localhost (::ffff:127.0.0.1)
        IpAddr::V6(addr) => {
            if addr.is_loopback() {
                return true;
            }
            // Check for IPv6-mapped IPv4 addresses
            if let Some(ipv4) = addr.to_ipv4_mapped() {
                return ipv4.is_loopback();
            }
            false
        }
    }
}

/// Check if a host string represents a LAN IP address
pub fn is_lan_host(host: &str) -> bool {
    // Strip port if present
    // For IPv4: "192.168.1.1:6969" -> "192.168.1.1"
    // For IPv6: "[fc00::1]:6969" -> "fc00::1" or "fc00::1" -> "fc00::1"
    let host_without_port = if host.starts_with('[') {
        // IPv6 with port: [ipv6]:port
        host.trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or(host)
    } else if host.contains("::") || host.chars().filter(|&c| c == ':').count() > 1 {
        // IPv6 without port (contains :: or multiple colons)
        host
    } else {
        // IPv4 or domain, strip after last colon
        host.split(':').next().unwrap_or(host)
    };

    // Try to parse as IP address
    if let Ok(ip) = host_without_port.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(addr) => {
                let octets = addr.octets();
                // 10.0.0.0/8
                if octets[0] == 10 {
                    return true;
                }
                // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
                if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
                    return true;
                }
                // 192.168.0.0/16
                if octets[0] == 192 && octets[1] == 168 {
                    return true;
                }
                false
            }
            IpAddr::V6(addr) => {
                // Check for IPv6-mapped IPv4 addresses
                if let Some(ipv4) = addr.to_ipv4_mapped() {
                    let octets = ipv4.octets();
                    if octets[0] == 10 {
                        return true;
                    }
                    if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
                        return true;
                    }
                    if octets[0] == 192 && octets[1] == 168 {
                        return true;
                    }
                }
                // IPv6 ULA (Unique Local Address) fc00::/7
                // These are the IPv6 equivalent of private addresses
                let segments = addr.segments();
                if segments[0] >= 0xfc00 && segments[0] <= 0xfdff {
                    return true;
                }
                // IPv6 link-local fe80::/10
                if segments[0] >= 0xfe80 && segments[0] <= 0xfebf {
                    return true;
                }
                false
            }
        }
    } else {
        // Not an IP address, likely a domain name
        false
    }
}

/// Detect the origin of a request
/// Localhost takes precedence over LAN
pub fn detect_origin(connect_info: &ConnectInfo<SocketAddr>, _host: Option<&str>) -> RequestOrigin {
    let ip = connect_info.0.ip();

    // First check if the actual socket connection is from localhost
    if is_local_request(connect_info) {
        return RequestOrigin::Localhost;
    }

    // Check if the socket IP (not Host header) is from LAN
    match ip {
        IpAddr::V4(addr) => {
            let octets = addr.octets();
            // 10.0.0.0/8
            if octets[0] == 10 {
                return RequestOrigin::Lan;
            }
            // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
            if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
                return RequestOrigin::Lan;
            }
            // 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 {
                return RequestOrigin::Lan;
            }
        }
        IpAddr::V6(addr) => {
            // Check for IPv6-mapped IPv4 addresses
            if let Some(ipv4) = addr.to_ipv4_mapped() {
                let octets = ipv4.octets();
                if octets[0] == 10 {
                    return RequestOrigin::Lan;
                }
                if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
                    return RequestOrigin::Lan;
                }
                if octets[0] == 192 && octets[1] == 168 {
                    return RequestOrigin::Lan;
                }
            }
            // IPv6 ULA (Unique Local Address) fc00::/7
            let segments = addr.segments();
            if segments[0] >= 0xfc00 && segments[0] <= 0xfdff {
                return RequestOrigin::Lan;
            }
            // IPv6 link-local fe80::/10
            if segments[0] >= 0xfe80 && segments[0] <= 0xfebf {
                return RequestOrigin::Lan;
            }
        }
    }

    // Otherwise, it's external
    RequestOrigin::External
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn make_connect_info(ip: IpAddr) -> ConnectInfo<SocketAddr> {
        ConnectInfo(SocketAddr::new(ip, 12345))
    }

    #[test]
    fn test_ipv4_localhost_detection() {
        let localhost = make_connect_info(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(is_local_request(&localhost));

        let localhost2 = make_connect_info(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)));
        assert!(is_local_request(&localhost2));
    }

    #[test]
    fn test_ipv6_localhost_detection() {
        let localhost = make_connect_info(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
        assert!(is_local_request(&localhost));
    }

    #[test]
    fn test_ipv6_mapped_ipv4_localhost() {
        // ::ffff:127.0.0.1
        let mapped = make_connect_info(IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001,
        )));
        assert!(is_local_request(&mapped));
    }

    #[test]
    fn test_lan_ip_detection() {
        // 10.x.x.x
        assert!(is_lan_host("10.0.0.1"));
        assert!(is_lan_host("10.255.255.255"));

        // 172.16-31.x.x
        assert!(is_lan_host("172.16.0.1"));
        assert!(is_lan_host("172.31.255.255"));
        assert!(!is_lan_host("172.15.0.1")); // Below range
        assert!(!is_lan_host("172.32.0.1")); // Above range

        // 192.168.x.x
        assert!(is_lan_host("192.168.1.1"));
        assert!(is_lan_host("192.168.255.255"));
    }

    #[test]
    fn test_lan_ip_with_port() {
        assert!(is_lan_host("192.168.1.1:6969"));
        assert!(is_lan_host("10.0.0.1:8080"));
    }

    #[test]
    fn test_external_detection() {
        assert!(!is_lan_host("1.2.3.4"));
        assert!(!is_lan_host("8.8.8.8"));
        assert!(!is_lan_host("example.com"));
        assert!(!is_lan_host("felix-salita.ngrok.app"));
    }

    #[test]
    fn test_detect_origin_localhost() {
        let localhost = make_connect_info(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(detect_origin(&localhost, None), RequestOrigin::Localhost);
        // Localhost takes precedence even with LAN host header
        assert_eq!(
            detect_origin(&localhost, Some("192.168.1.1")),
            RequestOrigin::Localhost
        );
    }

    #[test]
    fn test_detect_origin_lan() {
        let lan = make_connect_info(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)));
        assert_eq!(detect_origin(&lan, Some("192.168.1.1")), RequestOrigin::Lan);
    }

    #[test]
    fn test_detect_origin_external() {
        let external = make_connect_info(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        assert_eq!(
            detect_origin(&external, Some("example.com")),
            RequestOrigin::External
        );
        assert_eq!(detect_origin(&external, None), RequestOrigin::External);
    }

    #[test]
    fn test_ipv6_ula_detection() {
        // fc00::/7 range
        assert!(is_lan_host("fc00::1"));
        assert!(is_lan_host("fd00::1"));
    }

    #[test]
    fn test_ipv6_link_local_detection() {
        // fe80::/10 range
        assert!(is_lan_host("fe80::1"));
    }
}
