//! Download HTTP(S) restrito: sem rede privada, com teto de bytes e redirecionamento manual.

use std::io::{self, Read};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use url::Url;

pub struct FetchPolicy {
    pub allow_private: bool,
    pub max_bytes: u64,
    pub timeout: Duration,
    pub max_redirects: u8,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        FetchPolicy {
            allow_private: false,
            max_bytes: crate::image::MAX_IMAGE_BYTES,
            timeout: Duration::from_secs(8),
            max_redirects: 3,
        }
    }
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let seg0 = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00
                || (seg0 & 0xffc0) == 0xfe80)
        }
    }
}

/// Filtra os endereços na hora da resolução; o `ureq` conecta só no que sai daqui,
/// o que fecha a janela de DNS rebinding.
struct PolicyResolver {
    allow_private: bool,
}

impl ureq::Resolver for PolicyResolver {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let addrs: Vec<SocketAddr> = netloc
            .to_socket_addrs()?
            .filter(|a| self.allow_private || is_public_ip(a.ip()))
            .collect();
        if addrs.is_empty() {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "endereço bloqueado (rede privada)"))
        } else {
            Ok(addrs)
        }
    }
}

fn check_url(u: &Url) -> Result<(), String> {
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!("esquema não permitido: {}", u.scheme()));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("URL com credenciais não é permitida".into());
    }
    Ok(())
}

pub fn fetch(url: &str, policy: &FetchPolicy) -> Result<Vec<u8>, String> {
    let mut current = Url::parse(url).map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(policy.timeout)
        .resolver(PolicyResolver { allow_private: policy.allow_private })
        .build();
    for _ in 0..=policy.max_redirects {
        check_url(&current)?;
        let resp = agent.get(current.as_str()).call().map_err(|e| e.to_string())?;
        if (300..400).contains(&resp.status()) {
            let loc = resp.header("location").ok_or("redirecionamento sem Location")?;
            current = current.join(loc).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(len) = resp.header("content-length").and_then(|v| v.parse::<u64>().ok()) {
            if len > policy.max_bytes {
                return Err("resposta grande demais".into());
            }
        }
        let mut buf = Vec::new();
        resp.into_reader()
            .take(policy.max_bytes + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        if buf.len() as u64 > policy.max_bytes {
            return Err("resposta grande demais".into());
        }
        return Ok(buf);
    }
    Err("redirecionamentos demais".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serve_once(resp: Vec<u8>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(&resp);
            }
        });
        format!("http://127.0.0.1:{port}/img.png")
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn permissive() -> FetchPolicy {
        FetchPolicy { allow_private: true, ..FetchPolicy::default() }
    }

    #[test]
    fn ips_privados_sao_bloqueados() {
        for ip in ["127.0.0.1", "10.0.0.5", "172.16.3.1", "192.168.1.1", "169.254.169.254", "0.0.0.0", "0.1.2.3", "100.64.0.1", "::1", "fc00::1", "fe80::1", "::ffff:127.0.0.1"] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn baixa_corpo_de_servidor_local_quando_permitido() {
        let url = serve_once(ok(b"abc"));
        assert_eq!(fetch(&url, &permissive()).unwrap(), b"abc");
    }

    #[test]
    fn bloqueia_loopback_por_padrao() {
        let url = serve_once(ok(b"abc"));
        assert!(fetch(&url, &FetchPolicy::default()).is_err());
    }

    #[test]
    fn rejeita_corpo_maior_que_o_teto() {
        let url = serve_once(ok(&[0u8; 100]));
        let p = FetchPolicy { max_bytes: 10, ..permissive() };
        assert!(fetch(&url, &p).unwrap_err().contains("grande"));
    }

    #[test]
    fn rejeita_esquemas_nao_http() {
        for u in ["file:///etc/passwd", "ftp://x/y", "javascript:alert(1)", "http://user:pw@host/x"] {
            assert!(fetch(u, &permissive()).is_err(), "{u}");
        }
    }
}
