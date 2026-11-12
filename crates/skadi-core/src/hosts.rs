//! Hosts dosyası engelleme listesi. Skadi yalnızca kendi işaretli bloğunu ekler ve kaldırır;
//! dosyanın geri kalanına dokunmaz. Liste satırları sıkı biçimde doğrulanır ve işletim sisteminin
//! kendi hizmetlerine ait alan adları (güncelleme, etkinleştirme) hiçbir zaman engellenmez.

pub const BEGIN: &str = "# >>> skadi block list (remove with: skadi hosts unblock)";
pub const END: &str = "# <<< skadi block list";
pub const MAX_ENTRIES: usize = 400_000;

/// Engellenirse güncelleme ve sistem hizmetlerini bozabilecek alan adı sonekleri. Microsoft
/// Defender da bu tür kayıtları "HostsFileHijack" olarak işaretler.
const PROTECTED_SUFFIXES: [&str; 16] = [
    "microsoft.com",
    "windows.com",
    "windowsupdate.com",
    "windows.net",
    "msftncsi.com",
    "msftconnecttest.com",
    "live.com",
    "office.com",
    "apple.com",
    "icloud.com",
    "mzstatic.com",
    "ubuntu.com",
    "canonical.com",
    "debian.org",
    "fedoraproject.org",
    "archlinux.org",
];

pub fn is_domain(name: &str) -> bool {
    name.len() <= 253
        && name.contains('.')
        && !name.starts_with(['.', '-'])
        && !name.ends_with(['.', '-'])
        && name.split('.').all(|label| !label.is_empty() && label.len() <= 63)
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

fn is_protected(domain: &str) -> bool {
    PROTECTED_SUFFIXES
        .iter()
        .any(|s| domain == *s || domain.ends_with(&format!(".{s}")))
}

/// Bir hosts biçimli listedeki engellenecek alan adları (`0.0.0.0 alan` ya da `127.0.0.1 alan`).
pub fn parse_block_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        let (Some(address), Some(domain), None) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        let domain = domain.to_ascii_lowercase();
        if matches!(address, "0.0.0.0" | "127.0.0.1")
            && is_domain(&domain)
            && !domain.starts_with("localhost")
            && !is_protected(&domain)
        {
            out.push(domain);
        }
        if out.len() >= MAX_ENTRIES {
            break;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Skadi bloğu çıkarılmış hosts metni.
pub fn without_block(hosts: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in hosts.lines() {
        if line.trim() == BEGIN {
            inside = true;
        } else if line.trim() == END {
            inside = false;
        } else if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Eski Skadi bloğunu kaldırıp yenisini sona ekler.
pub fn with_block(hosts: &str, domains: &[String]) -> String {
    let mut out = without_block(hosts);
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(BEGIN);
    out.push('\n');
    for domain in domains {
        out.push_str("0.0.0.0 ");
        out.push_str(domain);
        out.push('\n');
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// Bloktaki kayıt sayısı; blok yoksa `None`.
pub fn block_size(hosts: &str) -> Option<usize> {
    let start = hosts.lines().position(|l| l.trim() == BEGIN)?;
    let lines: Vec<&str> = hosts.lines().skip(start + 1).collect();
    let end = lines.iter().position(|l| l.trim() == END)?;
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_round_trips() {
        let list = "# comment\n0.0.0.0 ads.example.com\n127.0.0.1 localhost\n0.0.0.0 settings-win.data.microsoft.com\n\
                    0.0.0.0 Tracker.Example.net # inline\n0.0.0.0 bad;rm\n0.0.0.0 a b c\n1.2.3.4 evil.example\n";
        let domains = parse_block_list(list);
        assert_eq!(domains, ["ads.example.com", "tracker.example.net"]);
        let original = "127.0.0.1 localhost\n::1 localhost\n";
        let blocked = with_block(original, &domains);
        assert_eq!(block_size(&blocked), Some(2));
        assert_eq!(without_block(&blocked).trim_end(), original.trim_end());
        // Yeniden engelleme eski bloğu değiştirir, çoğaltmaz.
        let again = with_block(&blocked, &domains[..1]);
        assert_eq!(block_size(&again), Some(1));
        assert_eq!(again.matches(BEGIN).count(), 1);
    }
}
