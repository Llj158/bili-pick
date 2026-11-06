use std::ffi::c_void;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_RAW_HEADERS_CRLF, WINHTTP_QUERY_STATUS_CODE,
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts,
};

const USER_AGENT: &str = "BiliPick/0.1";
const READ_BUFFER_SIZE: usize = 128 * 1024;

#[derive(Debug)]
pub enum HttpError {
    Request(String),
    Status { status: u16, message: String },
    Cancelled,
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(message) => write!(formatter, "{message}"),
            Self::Status { status, message } => write!(
                formatter,
                "HTTP {status}{}",
                if message.is_empty() {
                    String::new()
                } else {
                    format!(": {message}")
                }
            ),
            Self::Cancelled => formatter.write_str("已取消"),
        }
    }
}

impl std::error::Error for HttpError {}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: String,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        header_value(&self.headers, name)
    }
}

#[derive(Default)]
pub struct HttpClient {
    cookie: String,
}

impl HttpClient {
    pub fn has_cookies(&self) -> bool {
        !self.cookie.is_empty()
    }

    pub fn cookie_header(&self) -> &str {
        &self.cookie
    }

    pub fn set_cookie_header(&mut self, cookie: String) -> Result<(), String> {
        if cookie.len() > 32 * 1024 || cookie.contains(['\r', '\n', '\0']) {
            return Err("保存的登录记录格式无效".to_string());
        }
        self.cookie = cookie;
        Ok(())
    }

    pub fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpError> {
        let handles = open_request(url, "GET", None, headers, &self.cookie)?;
        let status = response_status(handles.request)?;
        let response_headers = response_headers(handles.request)?;
        let mut body = Vec::new();
        read_response_to(
            handles.request,
            &mut body,
            &AtomicBool::new(false),
            |_, _| {},
        )?;
        if credential_url(url) {
            self.update_cookies(&response_headers);
        }
        Ok(HttpResponse {
            status,
            body,
            headers: response_headers,
        })
    }

    pub fn download_to(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        path: &Path,
        cancel: &AtomicBool,
        progress: impl FnMut(u64, Option<u64>),
    ) -> Result<(), HttpError> {
        let offset = std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let range = format!("bytes={offset}-");
        let mut request_headers = headers.to_vec();
        if offset > 0 {
            request_headers.push(("Range", &range));
        }
        let handles = open_request(url, "GET", None, &request_headers, &self.cookie)?;
        let status = response_status(handles.request)?;
        let response_headers = response_headers(handles.request)?;
        if credential_url(url) {
            self.update_cookies(&response_headers);
        }

        let mut file = match status {
            206 if offset > 0 => {
                let expected = format!("bytes {offset}-");
                let content_range = header_value(&response_headers, "content-range")
                    .ok_or_else(|| HttpError::Request("续传响应缺少 Content-Range".to_string()))?;
                if !content_range.starts_with(&expected) {
                    return Err(HttpError::Request(format!(
                        "续传范围不匹配：期望 {expected}，实际 {content_range}"
                    )));
                }
                OpenOptions::new().append(true).open(path)
            }
            206 => OpenOptions::new().create(true).append(true).open(path),
            200 => OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path),
            _ => {
                let mut body = Vec::new();
                let _ = read_response_to(handles.request, &mut body, cancel, |_, _| {});
                return Err(HttpError::Status {
                    status,
                    message: String::from_utf8_lossy(&body).chars().take(200).collect(),
                });
            }
        }
        .map_err(|error| HttpError::Request(format!("无法打开临时文件: {error}")))?;

        let response_length = header_value(&response_headers, "content-length")
            .and_then(|value| value.parse::<u64>().ok());
        let total = response_length.map(|length| offset + length);
        let mut current = offset;
        read_response_into_file(
            handles.request,
            &mut file,
            &mut current,
            total,
            cancel,
            progress,
        )
    }

    fn update_cookies(&mut self, headers: &str) {
        for line in headers.lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if !name.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            let Some(pair) = value.trim().split(';').next() else {
                continue;
            };
            let Some((cookie_name, cookie_value)) = pair.split_once('=') else {
                continue;
            };
            replace_cookie(&mut self.cookie, cookie_name.trim(), cookie_value.trim());
        }
    }
}

fn replace_cookie(jar: &mut String, name: &str, value: &str) {
    let mut pairs: Vec<String> = jar
        .split(';')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(str::to_string)
        .collect();
    pairs.retain(|pair| pair.split_once('=').is_none_or(|(key, _)| key != name));
    if !value.is_empty() {
        pairs.push(format!("{name}={value}"));
    }
    *jar = pairs.join("; ");
}

struct RequestHandles {
    session: *mut c_void,
    connection: *mut c_void,
    request: *mut c_void,
}

impl Drop for RequestHandles {
    fn drop(&mut self) {
        unsafe {
            if !self.request.is_null() {
                WinHttpCloseHandle(self.request);
            }
            if !self.connection.is_null() {
                WinHttpCloseHandle(self.connection);
            }
            if !self.session.is_null() {
                WinHttpCloseHandle(self.session);
            }
        }
    }
}

fn open_request(
    url: &str,
    method: &str,
    body: Option<&[u8]>,
    headers: &[(&str, &str)],
    cookie: &str,
) -> Result<RequestHandles, HttpError> {
    let parsed = parse_url(url)?;
    let agent = wide(USER_AGENT);
    let host = wide(&parsed.host);
    let path = wide(&parsed.path);
    let method = wide(method);
    let session = unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        )
    };
    if session.is_null() {
        return Err(last_error("无法创建 WinHTTP 会话"));
    }
    unsafe {
        WinHttpSetTimeouts(session, 10_000, 15_000, 30_000, 60_000);
    }
    let connection = unsafe { WinHttpConnect(session, host.as_ptr(), parsed.port, 0) };
    if connection.is_null() {
        unsafe { WinHttpCloseHandle(session) };
        return Err(last_error("无法连接服务器"));
    }
    let flags: WINHTTP_OPEN_REQUEST_FLAGS = if parsed.secure {
        WINHTTP_FLAG_SECURE
    } else {
        0
    };
    let request = unsafe {
        WinHttpOpenRequest(
            connection,
            method.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            null(),
            flags,
        )
    };
    if request.is_null() {
        unsafe {
            WinHttpCloseHandle(connection);
            WinHttpCloseHandle(session);
        }
        return Err(last_error("无法创建 HTTP 请求"));
    }

    let cookie = if credential_url(url) { cookie } else { "" };
    if !cookie.is_empty() {
        // Do not forward an authenticated request to a redirect destination.
        let policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        if unsafe {
            WinHttpSetOption(
                request,
                WINHTTP_OPTION_REDIRECT_POLICY,
                (&policy as *const u32).cast(),
                4,
            )
        } == 0
        {
            let error = last_error("无法限制登录请求的重定向");
            drop(RequestHandles {
                session,
                connection,
                request,
            });
            return Err(error);
        }
    }
    let rendered_headers = render_headers(headers, cookie);
    let request_headers = wide(&rendered_headers);
    let header_pointer = if rendered_headers.is_empty() {
        null()
    } else {
        request_headers.as_ptr()
    };
    let payload = body.unwrap_or_default();
    let payload_pointer = if payload.is_empty() {
        null()
    } else {
        payload.as_ptr().cast()
    };
    let sent = unsafe {
        WinHttpSendRequest(
            request,
            header_pointer,
            request_headers.len().saturating_sub(1) as u32,
            payload_pointer,
            payload.len() as u32,
            payload.len() as u32,
            0,
        )
    };
    if sent == 0 {
        let error = last_error("HTTP 请求发送失败");
        unsafe {
            WinHttpCloseHandle(request);
            WinHttpCloseHandle(connection);
            WinHttpCloseHandle(session);
        }
        return Err(error);
    }
    let received = unsafe { WinHttpReceiveResponse(request, null_mut()) };
    if received == 0 {
        let error = last_error("接收 HTTP 响应失败");
        unsafe {
            WinHttpCloseHandle(request);
            WinHttpCloseHandle(connection);
            WinHttpCloseHandle(session);
        }
        return Err(error);
    }
    Ok(RequestHandles {
        session,
        connection,
        request,
    })
}

fn render_headers(headers: &[(&str, &str)], cookie: &str) -> String {
    let mut lines: Vec<String> = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect();
    if !cookie.is_empty() {
        lines.push(format!("Cookie: {cookie}"));
    }
    lines.join("\r\n")
}

fn credential_url(url: &str) -> bool {
    parse_url(url).is_ok_and(|parsed| {
        parsed.secure
            && parsed.port == 443
            && matches!(
                parsed.host.to_ascii_lowercase().as_str(),
                "api.bilibili.com"
                    | "passport.bilibili.com"
                    | "www.bilibili.com"
                    | "account.bilibili.com"
            )
    })
}

fn response_status(request: *mut c_void) -> Result<u16, HttpError> {
    let mut status = 0_u32;
    let mut length = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status as *mut u32).cast(),
            &mut length,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(last_error("无法读取 HTTP 状态码"));
    }
    Ok(status as u16)
}

fn response_headers(request: *mut c_void) -> Result<String, HttpError> {
    let mut length = 0_u32;
    unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            null(),
            null_mut(),
            &mut length,
            null_mut(),
        );
    }
    if length == 0 {
        return Err(last_error("无法读取 HTTP 响应头"));
    }
    let mut buffer = vec![0_u16; length as usize / 2 + 1];
    let mut capacity = (buffer.len() * 2) as u32;
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            null(),
            buffer.as_mut_ptr().cast(),
            &mut capacity,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(last_error("无法读取 HTTP 响应头"));
    }
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    Ok(String::from_utf16_lossy(&buffer))
}

fn read_response_to(
    request: *mut c_void,
    output: &mut Vec<u8>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), HttpError> {
    let mut chunk = vec![0_u8; READ_BUFFER_SIZE];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(HttpError::Cancelled);
        }
        let available = available_bytes(request)?;
        if available == 0 {
            break;
        }
        let read = read_chunk(request, &mut chunk, available)?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&chunk[..read]);
        progress(output.len() as u64, None);
    }
    Ok(())
}

fn read_response_into_file(
    request: *mut c_void,
    file: &mut File,
    current: &mut u64,
    total: Option<u64>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), HttpError> {
    let mut chunk = vec![0_u8; READ_BUFFER_SIZE];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(HttpError::Cancelled);
        }
        let available = available_bytes(request)?;
        if available == 0 {
            break;
        }
        let read = read_chunk(request, &mut chunk, available)?;
        if read == 0 {
            break;
        }
        file.write_all(&chunk[..read])
            .map_err(|error| HttpError::Request(format!("写入下载文件失败: {error}")))?;
        *current += read as u64;
        progress(*current, total);
    }
    file.flush()
        .map_err(|error| HttpError::Request(format!("刷新下载文件失败: {error}")))
}

fn available_bytes(request: *mut c_void) -> Result<u32, HttpError> {
    let mut available = 0_u32;
    let ok = unsafe { WinHttpQueryDataAvailable(request, &mut available) };
    if ok == 0 {
        return Err(last_error("读取网络数据失败"));
    }
    Ok(available)
}

fn read_chunk(request: *mut c_void, buffer: &mut [u8], available: u32) -> Result<usize, HttpError> {
    let limit = available.min(buffer.len() as u32);
    let mut read = 0_u32;
    let ok = unsafe { WinHttpReadData(request, buffer.as_mut_ptr().cast(), limit, &mut read) };
    if ok == 0 {
        return Err(last_error("读取网络数据失败"));
    }
    Ok(read as usize)
}

fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name
            .trim()
            .eq_ignore_ascii_case(name)
            .then_some(value.trim())
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
    secure: bool,
}

fn parse_url(url: &str) -> Result<ParsedUrl, HttpError> {
    let (scheme, remainder) = url
        .split_once("://")
        .ok_or_else(|| HttpError::Request("URL 缺少协议".to_string()))?;
    let secure = match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => false,
        _ => return Err(HttpError::Request(format!("不支持的 URL 协议: {scheme}"))),
    };
    let split_at = remainder.find('/').unwrap_or(remainder.len());
    let authority = &remainder[..split_at];
    let path = if split_at == remainder.len() {
        "/".to_string()
    } else {
        remainder[split_at..].to_string()
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, raw_port)) if raw_port.bytes().all(|byte| byte.is_ascii_digit()) => (
            host,
            raw_port
                .parse()
                .map_err(|_| HttpError::Request("URL 端口无效".to_string()))?,
        ),
        _ => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err(HttpError::Request("URL 缺少主机名".to_string()));
    }
    Ok(ParsedUrl {
        host: host.to_string(),
        port,
        path,
        secure,
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(context: &str) -> HttpError {
    HttpError::Request(format!("{context}（Windows 错误 {}）", unsafe {
        GetLastError()
    }))
}

#[cfg(test)]
mod tests {
    use super::{HttpClient, parse_url};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn credentials_are_scoped_to_official_https_endpoints() {
        for url in [
            "https://api.bilibili.com/x/test",
            "https://passport.bilibili.com/x/login",
        ] {
            assert!(super::credential_url(url));
        }
        for url in [
            "http://api.bilibili.com/x/test",
            "https://api.bilibili.com.evil.test/x",
            "https://cdn.bilivideo.com/media",
            "https://example.test/",
        ] {
            assert!(!super::credential_url(url));
        }
        assert!(
            HttpClient::default()
                .set_cookie_header("SESSDATA=x\r\nX-Test: y".into())
                .is_err()
        );
    }

    fn temp_path(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bilipick-{label}-{}-{stamp}.part",
            std::process::id()
        ))
    }

    fn serve_once(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            stream
                .write_all(response.as_bytes())
                .expect("write test response");
        });
        format!("http://{address}/media")
    }

    #[test]
    fn appends_when_server_returns_206() {
        let path = temp_path("append");
        fs::write(&path, b"abc").expect("seed partial file");
        let url = serve_once(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 3-7/8\r\nConnection: close\r\n\r\nworld",
        );
        let cancel = AtomicBool::new(false);
        let mut client = HttpClient::default();
        client
            .download_to(&url, &[], &path, &cancel, |_, _| {})
            .expect("resume completes");
        assert_eq!(fs::read(&path).expect("read result"), b"abcworld");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn restarts_when_range_is_ignored_with_200() {
        let path = temp_path("restart");
        fs::write(&path, b"stale").expect("seed partial file");
        let url =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nfresh");
        let cancel = AtomicBool::new(false);
        let mut client = HttpClient::default();
        client
            .download_to(&url, &[], &path, &cancel, |_, _| {})
            .expect("restart completes");
        assert_eq!(fs::read(&path).expect("read result"), b"fresh");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn parses_http_url_into_winhttp_components() {
        assert_eq!(
            parse_url("https://api.bilibili.com/x/test?q=1").expect("valid URL"),
            super::ParsedUrl {
                host: "api.bilibili.com".to_string(),
                port: 443,
                path: "/x/test?q=1".to_string(),
                secure: true,
            }
        );
    }
}
