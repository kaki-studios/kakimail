use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Mutex, OnceLock},
};

use anyhow::{anyhow, Result};
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use mailparse::MailHeaderMap;

use crate::smtp_common::Mail;

static TXT_CACHE: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    Pass,
    Fail,
    SoftFail,
    Neutral,
    None,
}

#[derive(Debug, Clone)]
pub struct IncomingAuthResult {
    pub spf: AuthStatus,
    pub dkim: AuthStatus,
    pub dmarc: AuthStatus,
    pub reject: bool,
}

pub async fn verify_incoming_mail(mail: &Mail, peer_ip: IpAddr) -> IncomingAuthResult {
    match verify_incoming_mail_inner(mail, peer_ip).await {
        Ok(result) => result,
        Err(err) => {
            tracing::warn!("mail authentication check failed open: {}", err);
            IncomingAuthResult {
                spf: AuthStatus::Neutral,
                dkim: AuthStatus::Neutral,
                dmarc: AuthStatus::Neutral,
                reject: false,
            }
        }
    }
}

async fn verify_incoming_mail_inner(mail: &Mail, peer_ip: IpAddr) -> Result<IncomingAuthResult> {
    let resolver = AuthDns::new();
    let envelope_domain =
        address_domain(&mail.from).ok_or_else(|| anyhow!("missing MAIL FROM domain"))?;
    let header_domain = header_from_domain(&mail.data).unwrap_or_else(|| envelope_domain.clone());
    let spf = check_spf(&resolver, &envelope_domain, peer_ip, 0).await;
    let dkim = check_dkim_header(&mail.data);
    let dmarc = check_dmarc(&resolver, &header_domain, &envelope_domain, spf, dkim).await;
    let reject = dmarc == AuthStatus::Fail;
    Ok(IncomingAuthResult {
        spf,
        dkim,
        dmarc,
        reject,
    })
}

struct AuthDns {
    resolver: hickory_resolver::TokioAsyncResolver,
}

impl AuthDns {
    fn new() -> Self {
        Self {
            resolver: hickory_resolver::TokioAsyncResolver::tokio(
                ResolverConfig::default(),
                ResolverOpts::default(),
            ),
        }
    }

    async fn txt(&self, name: &str) -> Result<Vec<String>> {
        let key = name.trim_end_matches('.').to_ascii_lowercase();
        if let Some(cached) = TXT_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
        {
            return Ok(cached);
        }

        let lookup = self.resolver.txt_lookup(format!("{}.", key)).await?;
        let records = lookup
            .iter()
            .map(|txt| {
                txt.txt_data()
                    .iter()
                    .map(|part| String::from_utf8_lossy(part).to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        if let Ok(mut cache) = TXT_CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() {
            cache.insert(key, records.clone());
        }
        Ok(records)
    }

    async fn lookup_ips(&self, name: &str) -> Vec<IpAddr> {
        self.resolver
            .lookup_ip(format!("{}.", name.trim_end_matches('.')))
            .await
            .map(|lookup| lookup.iter().collect())
            .unwrap_or_default()
    }

    async fn lookup_mx_ips(&self, domain: &str) -> Vec<IpAddr> {
        let mut ips = Vec::new();
        if let Ok(mx) = self
            .resolver
            .mx_lookup(format!("{}.", domain.trim_end_matches('.')))
            .await
        {
            for record in mx.iter() {
                ips.extend(self.lookup_ips(&record.exchange().to_string()).await);
            }
        }
        ips
    }
}

async fn check_spf(resolver: &AuthDns, domain: &str, ip: IpAddr, depth: usize) -> AuthStatus {
    if depth > 10 {
        return AuthStatus::Neutral;
    }
    let Ok(records) = resolver.txt(domain).await else {
        return AuthStatus::None;
    };
    let Some(record) = records
        .iter()
        .find(|txt| txt.to_ascii_lowercase().starts_with("v=spf1"))
    else {
        return AuthStatus::None;
    };

    for mechanism in record.split_whitespace().skip(1) {
        if mechanism.starts_with("redirect=") {
            let redirect = mechanism.trim_start_matches("redirect=");
            return Box::pin(check_spf(resolver, redirect, ip, depth + 1)).await;
        }
        let (qualifier, body) = match mechanism.as_bytes().first() {
            Some(b'+') | Some(b'-') | Some(b'~') | Some(b'?') => {
                (mechanism.as_bytes()[0] as char, &mechanism[1..])
            }
            _ => ('+', mechanism),
        };
        let matched = spf_mechanism_matches(resolver, domain, body, ip, depth).await;
        if matched {
            return match qualifier {
                '+' => AuthStatus::Pass,
                '-' => AuthStatus::Fail,
                '~' => AuthStatus::SoftFail,
                '?' => AuthStatus::Neutral,
                _ => AuthStatus::Neutral,
            };
        }
    }
    AuthStatus::Neutral
}

async fn spf_mechanism_matches(
    resolver: &AuthDns,
    domain: &str,
    mechanism: &str,
    ip: IpAddr,
    depth: usize,
) -> bool {
    if mechanism == "all" {
        return true;
    }
    if let Some(rest) = mechanism.strip_prefix("ip4:") {
        return ip_matches_cidr(ip, rest);
    }
    if let Some(rest) = mechanism.strip_prefix("ip6:") {
        return ip_matches_cidr(ip, rest);
    }
    if mechanism == "a" || mechanism.starts_with("a:") {
        let host = mechanism
            .strip_prefix("a:")
            .unwrap_or(domain)
            .split('/')
            .next()
            .unwrap_or(domain);
        return resolver
            .lookup_ips(host)
            .await
            .into_iter()
            .any(|candidate| candidate == ip);
    }
    if mechanism == "mx" || mechanism.starts_with("mx:") {
        let host = mechanism
            .strip_prefix("mx:")
            .unwrap_or(domain)
            .split('/')
            .next()
            .unwrap_or(domain);
        return resolver
            .lookup_mx_ips(host)
            .await
            .into_iter()
            .any(|candidate| candidate == ip);
    }
    if let Some(include) = mechanism.strip_prefix("include:") {
        return Box::pin(check_spf(resolver, include, ip, depth + 1)).await == AuthStatus::Pass;
    }
    false
}

async fn check_dmarc(
    resolver: &AuthDns,
    header_domain: &str,
    envelope_domain: &str,
    spf: AuthStatus,
    dkim: AuthStatus,
) -> AuthStatus {
    let record_name = format!("_dmarc.{}", header_domain);
    let Ok(records) = resolver.txt(&record_name).await else {
        return AuthStatus::None;
    };
    let Some(record) = records
        .iter()
        .find(|txt| txt.to_ascii_lowercase().starts_with("v=dmarc1"))
    else {
        return AuthStatus::None;
    };
    let policy = record
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("p=").map(str::to_ascii_lowercase))
        .unwrap_or_else(|| "none".to_string());
    let aligned_spf = spf == AuthStatus::Pass && domains_align(header_domain, envelope_domain);
    let aligned_dkim = dkim == AuthStatus::Pass;
    if aligned_spf || aligned_dkim || policy == "none" {
        AuthStatus::Pass
    } else if policy == "reject" || policy == "quarantine" {
        AuthStatus::Fail
    } else {
        AuthStatus::Neutral
    }
}

fn check_dkim_header(data: &str) -> AuthStatus {
    let Ok(parsed) = mailparse::parse_mail(data.as_bytes()) else {
        return AuthStatus::None;
    };
    if parsed.headers.get_first_value("DKIM-Signature").is_some() {
        // Full DKIM cryptographic verification needs RSA/Ed25519 verification support.
        // Keep the signal visible for DMARC, but fail open unless SPF+DMARC rejects.
        AuthStatus::Neutral
    } else {
        AuthStatus::None
    }
}

fn address_domain(address: &str) -> Option<String> {
    address
        .trim()
        .trim_matches('<')
        .trim_matches('>')
        .rsplit_once('@')
        .map(|(_, domain)| domain.trim().trim_end_matches('>').to_ascii_lowercase())
}

fn header_from_domain(data: &str) -> Option<String> {
    let parsed = mailparse::parse_mail(data.as_bytes()).ok()?;
    let from = parsed.headers.get_first_value("From")?;
    address_domain(&from)
}

fn domains_align(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || a.ends_with(&format!(".{}", b)) || b.ends_with(&format!(".{}", a))
}

fn ip_matches_cidr(ip: IpAddr, cidr: &str) -> bool {
    let (addr, prefix) = cidr.split_once('/').unwrap_or((cidr, ""));
    let Ok(network) = addr.parse::<IpAddr>() else {
        return false;
    };
    match (ip, network) {
        (IpAddr::V4(ip), IpAddr::V4(net)) => {
            let prefix = prefix.parse::<u32>().unwrap_or(32).min(32);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(ip) & mask) == (u32::from(net) & mask)
        }
        (IpAddr::V6(ip), IpAddr::V6(net)) => {
            let prefix = prefix.parse::<u32>().unwrap_or(128).min(128);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (u128::from(ip) & mask) == (u128::from(net) & mask)
        }
        _ => false,
    }
}
