use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkSource {
    Tailscale,
    WireGuard,
    ZeroTier,
    Vpn,
    Lan,
    Localhost,
}

impl NetworkSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tailscale => "tailscale",
            Self::WireGuard => "wireguard",
            Self::ZeroTier => "zerotier",
            Self::Vpn => "vpn",
            Self::Lan => "lan",
            Self::Localhost => "localhost",
        }
    }

    pub fn priority(&self) -> u8 {
        match self {
            Self::Tailscale => 1,
            Self::WireGuard => 2,
            Self::ZeroTier => 3,
            Self::Vpn => 4,
            Self::Lan => 5,
            Self::Localhost => 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpCandidate {
    pub ip: String,
    pub interface: String,
    pub source: NetworkSource,
    pub preferred: bool,
}

pub fn default_localhost_candidate(preferred: bool) -> IpCandidate {
    IpCandidate {
        ip: "127.0.0.1".to_string(),
        interface: "lo".to_string(),
        source: NetworkSource::Localhost,
        preferred,
    }
}

pub fn parse_ipv4_octets(ip: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }

    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        match part.parse::<u8>() {
            Ok(val) => octets[i] = val,
            Err(_) => return None,
        }
    }
    Some(octets)
}

pub fn is_cgnat_tailscale_ip(octets: &[u8; 4]) -> bool {
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

pub fn classify_interface(ifname: &str, ip: &str) -> NetworkSource {
    let if_lower = ifname.to_lowercase();
    let octets = parse_ipv4_octets(ip);

    if if_lower.starts_with("lo") || ip == "127.0.0.1" || ip.starts_with("127.") {
        return NetworkSource::Localhost;
    }

    if if_lower.starts_with("tailscale") {
        return NetworkSource::Tailscale;
    }

    if (if_lower.starts_with("utun") || if_lower.starts_with("tun"))
        && octets.as_ref().map(is_cgnat_tailscale_ip).unwrap_or(false)
    {
        return NetworkSource::Tailscale;
    }

    if if_lower.starts_with("wg") || if_lower.contains("wireguard") {
        return NetworkSource::WireGuard;
    }

    if if_lower.starts_with("zt") || if_lower.contains("zerotier") {
        return NetworkSource::ZeroTier;
    }

    if if_lower.starts_with("tun")
        || if_lower.starts_with("tap")
        || if_lower.starts_with("ppp")
        || if_lower.starts_with("utun")
        || if_lower.contains("vpn")
        || if_lower.contains("ipsec")
    {
        return NetworkSource::Vpn;
    }

    NetworkSource::Lan
}

#[derive(Deserialize)]
struct LinuxAddrInfo {
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    local: Option<String>,
}

#[derive(Deserialize)]
struct LinuxInterface {
    #[serde(default)]
    ifname: Option<String>,
    #[serde(default)]
    addr_info: Vec<LinuxAddrInfo>,
}

pub fn parse_ip_json(raw: &str) -> Vec<IpCandidate> {
    let parsed: Vec<LinuxInterface> = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut result = Vec::new();
    for iface in parsed {
        let ifname = match iface.ifname {
            Some(name) if !name.is_empty() => name,
            _ => continue,
        };

        for info in iface.addr_info {
            let is_inet = info.family.as_deref() == Some("inet");
            let local_ip = match info.local {
                Some(ip) if !ip.is_empty() && parse_ipv4_octets(&ip).is_some() => ip,
                _ => continue,
            };

            if is_inet {
                let source = classify_interface(&ifname, &local_ip);
                result.push(IpCandidate {
                    ip: local_ip,
                    interface: ifname.clone(),
                    source,
                    preferred: false,
                });
            }
        }
    }
    result
}

pub fn parse_ifconfig(raw: &str) -> Vec<IpCandidate> {
    let mut result = Vec::new();
    let mut current_iface: Option<String> = None;

    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }

        let is_indented = line.starts_with(' ') || line.starts_with('\t');
        if !is_indented {
            if let Some(colon_pos) = line.find(':') {
                let name = line[..colon_pos].trim();
                if !name.is_empty() {
                    current_iface = Some(name.to_string());
                }
            } else if let Some(space_pos) = line.find(' ') {
                let name = line[..space_pos].trim();
                if !name.is_empty() {
                    current_iface = Some(name.to_string());
                }
            }
            continue;
        }

        let Some(ref ifname) = current_iface else {
            continue;
        };

        let trimmed = line.trim();
        if trimmed.starts_with("inet ") || trimmed.starts_with("inet\t") {
            let tokens: Vec<&str> = trimmed.split_whitespace().collect();
            let mut extracted_ip: Option<String> = None;

            if tokens.len() >= 2 {
                if tokens[1].starts_with("addr:") {
                    extracted_ip = Some(tokens[1].trim_start_matches("addr:").to_string());
                } else {
                    extracted_ip = Some(tokens[1].to_string());
                }
            }

            if let Some(ip) = extracted_ip {
                let clean_ip = ip.split('/').next().unwrap_or("").to_string();
                if parse_ipv4_octets(&clean_ip).is_some() {
                    let source = classify_interface(ifname, &clean_ip);
                    result.push(IpCandidate {
                        ip: clean_ip,
                        interface: ifname.clone(),
                        source,
                        preferred: false,
                    });
                }
            }
        }
    }

    result
}

pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Option<String>;
}

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Option<String> {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    String::from_utf8(output.stdout).ok()
                } else {
                    None
                }
            })
    }
}

pub fn discover_ips_with<R: CommandRunner>(runner: &R) -> Vec<IpCandidate> {
    let mut candidates = Vec::new();

    if let Some(output) = runner.run("ip", &["-j", "addr"]) {
        candidates = parse_ip_json(&output);
    }

    if candidates.is_empty() {
        candidates = runner
            .run("ifconfig", &[])
            .map(|output| parse_ifconfig(&output))
            .unwrap_or_default();
    }

    let mut deduped: Vec<IpCandidate> = Vec::new();
    for c in candidates {
        if let Some(existing) = deduped.iter_mut().find(|e| e.ip == c.ip) {
            if c.source.priority() < existing.source.priority() {
                *existing = c;
            }
        } else {
            deduped.push(c);
        }
    }

    deduped.sort_by(|a, b| {
        a.source
            .priority()
            .cmp(&b.source.priority())
            .then_with(|| a.interface.cmp(&b.interface))
            .then_with(|| a.ip.cmp(&b.ip))
    });

    if !deduped.iter().any(|c| c.ip == "127.0.0.1") {
        deduped.push(default_localhost_candidate(false));
    }

    if let Some(first) = deduped.first_mut() {
        first.preferred = true;
    }

    deduped
}

pub fn discover_ips() -> Vec<IpCandidate> {
    discover_ips_with(&SystemCommandRunner)
}
