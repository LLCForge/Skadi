//! Gizlilik odaklı DNS sağlayıcıları: hız testi, durum, ayarlama ve sıfırlama.
//!
//! Komutlar yalnızca bu dosyadaki sabit sağlayıcı verisinden kurulur; kullanıcı girdisi hiçbir
//! komuta karışmaz (yalnızca sağlayıcı kimliği seçilir).

use std::{
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub ipv4: &'static [&'static str],
    pub ipv6: &'static [&'static str],
    /// DNS-over-TLS sunucu adı.
    pub tls_host: &'static str,
    /// DNS-over-HTTPS şablonu.
    pub doh: &'static str,
}

pub const PROVIDERS: [Provider; 4] = [
    Provider {
        id: "quad9",
        name: "Quad9",
        ipv4: &["9.9.9.9", "149.112.112.112"],
        ipv6: &["2620:fe::fe", "2620:fe::9"],
        tls_host: "dns.quad9.net",
        doh: "https://dns.quad9.net/dns-query",
    },
    Provider {
        id: "cloudflare",
        name: "Cloudflare",
        ipv4: &["1.1.1.1", "1.0.0.1"],
        ipv6: &["2606:4700:4700::1111", "2606:4700:4700::1001"],
        tls_host: "cloudflare-dns.com",
        doh: "https://cloudflare-dns.com/dns-query",
    },
    Provider {
        id: "mullvad",
        name: "Mullvad",
        ipv4: &["194.242.2.2"],
        ipv6: &["2a07:e340::2"],
        tls_host: "dns.mullvad.net",
        doh: "https://dns.mullvad.net/dns-query",
    },
    Provider {
        id: "adguard",
        name: "AdGuard",
        ipv4: &["94.140.14.14", "94.140.15.15"],
        ipv6: &["2a10:50c0::ad1:ff", "2a10:50c0::ad2:ff"],
        tls_host: "dns.adguard-dns.com",
        doh: "https://dns.adguard-dns.com/dns-query",
    },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id.eq_ignore_ascii_case(id))
}

/// Verilen adreslerden hangi sağlayıcıya ait olduğu.
pub fn identify(address: &str) -> Option<&'static Provider> {
    PROVIDERS
        .iter()
        .find(|p| p.ipv4.contains(&address) || p.ipv6.iter().any(|a| a.eq_ignore_ascii_case(address)))
}

/// `example.com` için A kaydı sorgusu (RFC 1035).
fn query_packet(id: u16) -> Vec<u8> {
    let mut packet = id.to_be_bytes().to_vec();
    // Özyineleme istenir; tek soru.
    packet.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for label in ["example", "com"] {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x01]);
    packet
}

/// Bir DNS sunucusunun yanıt süresi: üç denemenin en iyisi. Yanıt yoksa `None`.
pub fn measure(server: &str) -> Option<Duration> {
    let address: SocketAddr = format!("{server}:53").parse().ok()?;
    let socket = UdpSocket::bind(if address.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut best: Option<Duration> = None;
    for attempt in 0..3u16 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let id = (nanos as u16) ^ attempt.wrapping_mul(0x9E37);
        let packet = query_packet(id);
        let start = Instant::now();
        if socket.send_to(&packet, address).is_err() {
            continue;
        }
        let mut buffer = [0u8; 512];
        // Yalnızca aynı kimlikli ve sunucudan gelen yanıt sayılır.
        while let Ok((len, from)) = socket.recv_from(&mut buffer) {
            if from == address && len >= 12 && buffer[..2] == id.to_be_bytes() && buffer[2] & 0x80 != 0 {
                let elapsed = start.elapsed();
                best = Some(best.map_or(elapsed, |b| b.min(elapsed)));
                break;
            }
        }
    }
    best
}

/// Windows: etkin bağdaştırıcılarda DNS'i ayarlar; Windows 11'de DoH şablonunu da kaydeder.
pub fn windows_set_script(provider: &Provider, doh: bool) -> String {
    let servers: Vec<String> = provider
        .ipv4
        .iter()
        .chain(provider.ipv6)
        .map(|s| format!("'{s}'"))
        .collect();
    let list = servers.join(",");
    let mut script = format!(
        "$ErrorActionPreference = 'Stop'; $servers = @({list}); \
         Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object {{ \
         Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ServerAddresses $servers }}"
    );
    if doh {
        // Şifreli bağlantı kurulamazsa düz DNS'e sessizce dönülmez (gizlilik önceliği).
        script.push_str(&format!(
            "; foreach ($s in $servers) {{ if (Get-DnsClientDohServerAddress -ServerAddress $s -ErrorAction SilentlyContinue) {{ \
             Set-DnsClientDohServerAddress -ServerAddress $s -DohTemplate '{t}' -AllowFallbackToUdp $false -AutoUpgrade $true }} \
             else {{ Add-DnsClientDohServerAddress -ServerAddress $s -DohTemplate '{t}' -AllowFallbackToUdp $false -AutoUpgrade $true }} }}",
            t = provider.doh
        ));
    }
    script
}

pub const WINDOWS_RESET_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; Get-NetAdapter | Where-Object Status -eq 'Up' | \
ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ResetServerAddresses }";

pub const WINDOWS_STATUS_SCRIPT: &str = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-DnsClientServerAddress | Where-Object { $_.ServerAddresses } | \
ForEach-Object { $_.InterfaceAlias + '|' + ($_.ServerAddresses -join ' ') }";

/// systemd-resolved için `DNS=` değeri: DNS-over-TLS sunucu adıyla (`adres#ad`).
pub fn resolved_dns_value(provider: &Provider) -> String {
    provider
        .ipv4
        .iter()
        .chain(provider.ipv6)
        .map(|address| format!("{address}#{}", provider.tls_host))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_valid_query() {
        let packet = query_packet(0xABCD);
        assert_eq!(&packet[..2], &[0xAB, 0xCD]);
        assert_eq!(packet.len(), 12 + 13 + 4);
        assert_eq!(&packet[12..20], b"\x07example");
    }

    #[test]
    fn identifies_and_formats_providers() {
        assert_eq!(identify("9.9.9.9").map(|p| p.id), Some("quad9"));
        assert_eq!(identify("2606:4700:4700::1111").map(|p| p.id), Some("cloudflare"));
        assert!(identify("8.8.8.8").is_none());
        assert!(resolved_dns_value(provider("mullvad").unwrap()).starts_with("194.242.2.2#dns.mullvad.net"));
        let script = windows_set_script(provider("quad9").unwrap(), true);
        assert!(script.contains("'9.9.9.9'") && script.contains("https://dns.quad9.net/dns-query"));
        assert!(script.contains("-AllowFallbackToUdp $false"));
    }
}
