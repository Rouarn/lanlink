//! 极简 HTTP/1.1 客户端，只用于房间码解析服务。
//!
//! 支持 `http://` 与 `https://`（TLS 由 rustls 完成，默认开启证书校验）。
//! 不引入完整 HTTP 栈（ureq / reqwest），因为这里只需要「发一个请求、读回 JSON」，
//! 自己维护这一百多行的实现反而能精确区分 DNS / TLS / 超时 / 状态码错误。
//!
//! 对端是自建小服务，因此不做连接池、重定向与 chunked 之外的复杂处理。

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub enum HttpError {
    /// 域名解析失败（DNS 层面的问题，与网络是否连通无关）
    Dns(String),
    /// TCP 连不上（端口没开、被墙、路由不通等）
    Connect(String),
    /// 连接 / 读写超时
    Timeout(String),
    /// TLS 握手或证书校验失败（说明是证书/加密层的问题，不是网络不通）
    Tls(String),
    /// 服务返回了非 2xx 状态码
    Status(u16),
    /// 响应内容无法解析
    Body(String),
    /// 其它网络层问题
    Network(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Dns(message) => write!(f, "域名解析失败：{message}"),
            HttpError::Connect(message) => write!(f, "{message}"),
            HttpError::Timeout(message) => write!(f, "超时：{message}"),
            HttpError::Tls(message) => write!(f, "{message}"),
            HttpError::Status(code) => write!(f, "HTTP {code}"),
            HttpError::Body(message) => write!(f, "{message}"),
            HttpError::Network(message) => write!(f, "{message}"),
        }
    }
}

#[derive(Debug)]
struct Endpoint {
    https: bool,
    host: String,
    port: u16,
    prefix: String,
}

fn parse_base(base_url: &str) -> Result<Endpoint, HttpError> {
    let base = base_url.trim().trim_end_matches('/');
    let (https, rest) = if let Some(rest) = base.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = base.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(HttpError::Network(
            "解析服务地址只支持 http:// 或 https://".to_string(),
        ));
    };
    let (authority, prefix) = match rest.find('/') {
        Some(index) => (&rest[..index], rest[index..].to_string()),
        None => (rest, String::new()),
    };
    if authority.is_empty() {
        return Err(HttpError::Network("解析服务地址缺少主机名".to_string()));
    }
    let default_port = if https { 443u16 } else { 80u16 };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !host.ends_with(']') => (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|_| HttpError::Network(format!("端口 {port} 不合法")))?,
        ),
        _ => (authority.to_string(), default_port),
    };
    Ok(Endpoint {
        https,
        host,
        port,
        prefix,
    })
}

fn connect_tcp(endpoint: &Endpoint, timeout: Duration) -> Result<TcpStream, HttpError> {
    let target = format!("{}:{}", endpoint.host, endpoint.port);
    let addrs = target.to_socket_addrs().map_err(|err| {
        HttpError::Dns(format!("无法解析 {target}：{err}"))
    })?;
    let mut last_err = String::new();
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(timeout));
                let _ = stream.set_write_timeout(Some(timeout));
                return Ok(stream);
            }
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                return Err(HttpError::Timeout(format!(
                    "连接 {target} 超过 {}s",
                    timeout.as_secs()
                )));
            }
            Err(err) => last_err = err.to_string(),
        }
    }
    Err(HttpError::Connect(format!("无法连接 {target}：{last_err}")))
}

fn tls_config() -> &'static Arc<rustls::ClientConfig> {
    static CONFIG: std::sync::OnceLock<Arc<rustls::ClientConfig>> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        // 只信 webpki-roots 内置的公共根证书；不提供任何跳过校验的开关。
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("ring provider 支持默认协议版本")
            .with_root_certificates(roots)
            .with_no_client_auth();
        Arc::new(config)
    })
}

/// 把 rustls 的错误翻译成人能看懂的话，重点是区分「证书问题」和「网络不通」。
fn describe_tls_error(err: &rustls::Error) -> String {
    match err {
        rustls::Error::InvalidCertificate(cert) => {
            let hint = match cert {
                rustls::CertificateError::NotValidForName
                | rustls::CertificateError::NotValidForNameContext { .. } => {
                    "证书与访问的域名不匹配"
                }
                rustls::CertificateError::Expired => "证书已过期",
                rustls::CertificateError::NotValidYet => "证书还未生效",
                rustls::CertificateError::UnknownIssuer => {
                    "证书签发者不在系统信任列表里（自签名证书会这样）"
                }
                rustls::CertificateError::Revoked => "证书已被吊销",
                _ => "证书内容不被信任",
            };
            format!("TLS 证书校验未通过：{hint}。这是服务器证书的问题，不是网络不通")
        }
        rustls::Error::AlertReceived(alert) => {
            format!("TLS 握手被服务器拒绝（{alert:?}）")
        }
        other => format!("TLS 握手失败：{other}"),
    }
}

/// 把 io 错误里的 rustls 内层错误取出来，交给 [`describe_tls_error`]。
fn describe_tls_io_error(err: &std::io::Error) -> String {
    match err.get_ref() {
        Some(inner) => match inner.downcast_ref::<rustls::Error>() {
            Some(tls) => describe_tls_error(tls),
            None => format!("TLS 握手失败：{inner}"),
        },
        None => format!("TLS 握手失败：{err}"),
    }
}

fn wrap_tls(
    stream: TcpStream,
    host: &str,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, HttpError> {
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|err| HttpError::Tls(format!("主机名不合法（{host}）：{err}")))?;
    let mut connection = rustls::ClientConnection::new(tls_config().clone(), server_name)
        .map_err(|err| HttpError::Tls(describe_tls_error(&err)))?;
    let mut stream = stream;
    // 显式跑完握手，才能把证书错误从读写错误里区分出来
    connection
        .complete_io(&mut stream)
        .map_err(|err| HttpError::Tls(describe_tls_io_error(&err)))?;
    Ok(rustls::StreamOwned::new(connection, stream))
}

fn request(
    base_url: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
    timeout: Duration,
) -> Result<String, HttpError> {
    let endpoint = parse_base(base_url)?;
    let tcp = connect_tcp(&endpoint, timeout)?;

    let mut head = format!(
        "{method} {}{} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nAccept: application/json\r\n",
        endpoint.prefix, path, endpoint.host, endpoint.port
    );
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        head.push_str(&format!("X-LanLink-Token: {token}\r\n"));
    }
    let payload = body.unwrap_or("");
    if body.is_some() {
        head.push_str("Content-Type: application/json\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", payload.as_bytes().len()));
    }
    head.push_str("\r\n");
    head.push_str(payload);

    // http 直连 / https 走 rustls，之后统一按 Read + Write 处理
    let mut stream: Box<dyn ReadWrite> = if endpoint.https {
        Box::new(wrap_tls(tcp, &endpoint.host)?)
    } else {
        Box::new(tcp)
    };

    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|err| io_error("发送请求失败", err))?;

    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(count) => raw.extend_from_slice(&buf[..count]),
            // 已经收到内容后对端直接断开（https 常见：没发 close_notify），按读完处理
            Err(_) if !raw.is_empty() => break,
            Err(err) => return Err(io_error("读取响应失败", err)),
        }
    }

    let text = String::from_utf8_lossy(&raw).to_string();
    let (header, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| HttpError::Network("响应格式异常".to_string()))?;

    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| HttpError::Body("响应缺少状态码".to_string()))?;
    if !(200..300).contains(&status) {
        return Err(HttpError::Status(status));
    }
    Ok(body.to_string())
}

/// 读写超时与其它 io 错误分开报，避免「证书过期」和「网线没插」混成一句话。
fn io_error(context: &str, err: std::io::Error) -> HttpError {
    if matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        return HttpError::Timeout(format!("{context}：等待服务器响应超时"));
    }
    HttpError::Network(format!("{context}：{err}"))
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

pub fn get_json(
    base_url: &str,
    path: &str,
    token: Option<&str>,
    timeout: Duration,
) -> Result<serde_json::Value, HttpError> {
    let body = request(base_url, "GET", path, token, None, timeout)?;
    serde_json::from_str(&body).map_err(|err| HttpError::Body(format!("响应不是合法 JSON：{err}")))
}

pub fn post_json(
    base_url: &str,
    path: &str,
    token: Option<&str>,
    payload: &serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, HttpError> {
    let body = payload.to_string();
    let text = request(base_url, "POST", path, token, Some(&body), timeout)?;
    serde_json::from_str(&text).map_err(|err| HttpError::Body(format!("响应不是合法 JSON：{err}")))
}

#[cfg(test)]
mod tests {
    use super::{parse_base, HttpError};

    #[test]
    fn parses_https_with_default_port_and_prefix() {
        let endpoint = parse_base("https://lanlink.example.com/api/lanlink/").unwrap();
        assert!(endpoint.https);
        assert_eq!(endpoint.host, "lanlink.example.com");
        assert_eq!(endpoint.port, 443);
        assert_eq!(endpoint.prefix, "/api/lanlink");
    }

    #[test]
    fn parses_http_for_local_debugging() {
        let endpoint = parse_base("http://127.0.0.1:18080").unwrap();
        assert!(!endpoint.https);
        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 18080);
        assert_eq!(endpoint.prefix, "");
    }

    #[test]
    fn rejects_other_schemes() {
        for bad in ["ftp://host/x", "host:8080", "//host", ""] {
            match parse_base(bad) {
                Err(HttpError::Network(message)) => {
                    assert!(message.contains("http:// 或 https://"), "{bad} → {message}")
                }
                other => panic!("{bad} 应该被拒绝，实际 {other:?}"),
            }
        }
    }

    #[test]
    fn certificate_error_is_reported_as_certificate_problem() {
        let err = rustls::Error::InvalidCertificate(rustls::CertificateError::Expired);
        let text = super::describe_tls_error(&err);
        assert!(text.contains("证书"), "{text}");
        assert!(text.contains("不是网络不通"), "{text}");
        assert!(text.contains("已过期"), "{text}");
    }

    /// 真实 https 站点握手（默认忽略，用 `cargo test -- --ignored` 手动跑）
    #[test]
    #[ignore]
    fn real_https_site_handshake_works() {
        let url = std::env::var("LANLINK_TLS_OK_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string());
        match super::request(
            &url,
            "GET",
            "/zen",
            None,
            None,
            std::time::Duration::from_secs(10),
        ) {
            Ok(body) => assert!(!body.trim().is_empty(), "响应体不应为空"),
            // 能读到 HTTP 状态码就说明 TLS 握手与请求都通了（GitHub 对无 UA 请求返回 403）
            Err(HttpError::Status(_)) => {}
            Err(err) => panic!("https 握手应当成功，实际：{err}"),
        }
    }

    /// 本地 http 服务仍可用（默认忽略，配合 `packages/server` 手动跑）
    #[test]
    #[ignore]
    fn local_http_service_still_works() {
        let url = std::env::var("LANLINK_HTTP_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18080".to_string());
        let body = super::request(
            &url,
            "GET",
            "/healthz",
            None,
            None,
            std::time::Duration::from_secs(5),
        )
        .expect("本地 http 服务应当可用");
        assert!(body.contains("ok"), "{body}");
    }

    /// 自签 / 错误域名必须被拒绝（默认忽略）
    #[test]
    #[ignore]
    fn self_signed_certificate_is_rejected() {
        let url = std::env::var("LANLINK_TLS_BAD_URL")
            .expect("需要设置 LANLINK_TLS_BAD_URL，例如 https://127.0.0.1:18443");
        match super::request(&url, "GET", "/", None, None, std::time::Duration::from_secs(5)) {
            Err(HttpError::Tls(message)) => {
                eprintln!("[{url}] → {message}");
                assert!(message.contains("证书"), "{message}");
            }
            other => panic!("应当因证书校验失败被拒绝，实际 {other:?}"),
        }
    }
}
