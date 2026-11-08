use tandem::net_interfaces::{
    CommandRunner, NetworkSource, classify_interface, default_localhost_candidate,
    discover_ips_with, parse_ifconfig, parse_ip_json,
};

#[test]
fn test_classify_interface_sources() {
    assert_eq!(
        classify_interface("lo0", "127.0.0.1"),
        NetworkSource::Localhost
    );
    assert_eq!(
        classify_interface("lo", "127.0.0.1"),
        NetworkSource::Localhost
    );
    assert_eq!(
        classify_interface("eth0", "127.0.0.2"),
        NetworkSource::Localhost
    );

    assert_eq!(
        classify_interface("tailscale0", "100.80.1.2"),
        NetworkSource::Tailscale
    );
    assert_eq!(
        classify_interface("utun3", "100.64.0.5"),
        NetworkSource::Tailscale
    );
    assert_eq!(
        classify_interface("utun3", "100.127.255.254"),
        NetworkSource::Tailscale
    );

    assert_eq!(
        classify_interface("wg0", "10.0.0.2"),
        NetworkSource::WireGuard
    );
    assert_eq!(
        classify_interface("wireguard_tun", "192.168.99.2"),
        NetworkSource::WireGuard
    );

    assert_eq!(
        classify_interface("zt0", "10.147.17.1"),
        NetworkSource::ZeroTier
    );
    assert_eq!(
        classify_interface("ztbt3vj2", "192.168.192.5"),
        NetworkSource::ZeroTier
    );

    assert_eq!(classify_interface("tun0", "10.8.0.2"), NetworkSource::Vpn);
    assert_eq!(classify_interface("tap0", "10.9.0.2"), NetworkSource::Vpn);
    assert_eq!(classify_interface("ppp0", "10.2.0.2"), NetworkSource::Vpn);
    assert_eq!(
        classify_interface("utun1", "10.200.1.2"),
        NetworkSource::Vpn
    );

    assert_eq!(
        classify_interface("en0", "192.168.1.100"),
        NetworkSource::Lan
    );
    assert_eq!(classify_interface("eth0", "10.0.1.5"), NetworkSource::Lan);
    assert_eq!(
        classify_interface("wlan0", "172.16.0.4"),
        NetworkSource::Lan
    );
    assert_eq!(
        classify_interface("wlp2s0", "192.168.0.10"),
        NetworkSource::Lan
    );
    assert_eq!(
        classify_interface("enp3s0", "192.168.1.15"),
        NetworkSource::Lan
    );
}

#[test]
fn test_parse_linux_ip_json() {
    let sample = r#"[
        {
            "ifname": "lo",
            "addr_info": [
                {"family": "inet", "local": "127.0.0.1", "prefixlen": 8},
                {"family": "inet6", "local": "::1", "prefixlen": 128}
            ]
        },
        {
            "ifname": "eth0",
            "addr_info": [
                {"family": "inet", "local": "192.168.1.42", "prefixlen": 24}
            ]
        },
        {
            "ifname": "tailscale0",
            "addr_info": [
                {"family": "inet", "local": "100.90.10.5", "prefixlen": 32}
            ]
        },
        {
            "ifname": "wg0",
            "addr_info": [
                {"family": "inet", "local": "10.10.0.1", "prefixlen": 24}
            ]
        }
    ]"#;

    let candidates = parse_ip_json(sample);
    assert_eq!(candidates.len(), 4);

    let tailscale = candidates
        .iter()
        .find(|c| c.source == NetworkSource::Tailscale)
        .unwrap();
    assert_eq!(tailscale.ip, "100.90.10.5");
    assert_eq!(tailscale.interface, "tailscale0");

    let wireguard = candidates
        .iter()
        .find(|c| c.source == NetworkSource::WireGuard)
        .unwrap();
    assert_eq!(wireguard.ip, "10.10.0.1");

    let lan = candidates
        .iter()
        .find(|c| c.source == NetworkSource::Lan)
        .unwrap();
    assert_eq!(lan.ip, "192.168.1.42");

    let localhost = candidates
        .iter()
        .find(|c| c.source == NetworkSource::Localhost)
        .unwrap();
    assert_eq!(localhost.ip, "127.0.0.1");
}

#[test]
fn test_parse_macos_ifconfig() {
    let sample = r#"
lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384
	options=1203<RXCSUM,TXCSUM,TXSTATUS,SW_TIMESTAMP>
	inet 127.0.0.1 netmask 0xff000000 
	inet6 ::1 prefixlen 128 
en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500
	ether a4:83:e7:c4:90:12 
	inet6 fe80::1092:384a:8a12%en0 prefixlen 64 scopeid 0xc 
	inet 192.168.1.120 netmask 0xffffff00 broadcast 192.168.1.255
utun3: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1280
	inet 100.85.12.34 --> 100.85.12.34 netmask 0xffffffff 
utun4: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1420
	inet 10.6.0.2 --> 10.6.0.2 netmask 0xffffff00
"#;

    let candidates = parse_ifconfig(sample);
    assert_eq!(candidates.len(), 4);

    let ts = candidates.iter().find(|c| c.interface == "utun3").unwrap();
    assert_eq!(ts.ip, "100.85.12.34");
    assert_eq!(ts.source, NetworkSource::Tailscale);

    let vpn = candidates.iter().find(|c| c.interface == "utun4").unwrap();
    assert_eq!(vpn.ip, "10.6.0.2");
    assert_eq!(vpn.source, NetworkSource::Vpn);

    let lan = candidates.iter().find(|c| c.interface == "en0").unwrap();
    assert_eq!(lan.ip, "192.168.1.120");
    assert_eq!(lan.source, NetworkSource::Lan);

    let lo = candidates.iter().find(|c| c.interface == "lo0").unwrap();
    assert_eq!(lo.ip, "127.0.0.1");
    assert_eq!(lo.source, NetworkSource::Localhost);
}

#[derive(Default)]
struct MockRunner {
    ip_output: Option<String>,
    ifconfig_output: Option<String>,
}

impl CommandRunner for MockRunner {
    fn run(&self, program: &str, _args: &[&str]) -> Option<String> {
        match program {
            "ip" => self.ip_output.clone(),
            "ifconfig" => self.ifconfig_output.clone(),
            _ => None,
        }
    }
}

#[test]
fn test_discover_ips_sorting_and_preferred_selection() {
    let runner = MockRunner {
        ip_output: Some(
            r#"[
            {"ifname": "en0", "addr_info": [{"family": "inet", "local": "192.168.1.50"}]},
            {"ifname": "lo", "addr_info": [{"family": "inet", "local": "127.0.0.1"}]},
            {"ifname": "wg0", "addr_info": [{"family": "inet", "local": "10.0.0.5"}]},
            {"ifname": "tailscale0", "addr_info": [{"family": "inet", "local": "100.70.1.1"}]}
        ]"#
            .to_string(),
        ),
        ifconfig_output: None,
    };

    let discovered = discover_ips_with(&runner);
    assert_eq!(discovered.len(), 4);

    // Sorting order must place Tailscale first, then WireGuard, then LAN, then Localhost
    assert_eq!(discovered[0].source, NetworkSource::Tailscale);
    assert_eq!(discovered[0].ip, "100.70.1.1");
    assert!(discovered[0].preferred);

    assert_eq!(discovered[1].source, NetworkSource::WireGuard);
    assert_eq!(discovered[1].ip, "10.0.0.5");
    assert!(!discovered[1].preferred);

    assert_eq!(discovered[2].source, NetworkSource::Lan);
    assert_eq!(discovered[2].ip, "192.168.1.50");
    assert!(!discovered[2].preferred);

    assert_eq!(discovered[3].source, NetworkSource::Localhost);
    assert_eq!(discovered[3].ip, "127.0.0.1");
    assert!(!discovered[3].preferred);
}

#[test]
fn test_discover_ips_fallback_guarantees_localhost() {
    let empty_runner = MockRunner {
        ip_output: None,
        ifconfig_output: None,
    };

    let discovered = discover_ips_with(&empty_runner);
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0], default_localhost_candidate(true));
    assert!(discovered[0].preferred);
}
