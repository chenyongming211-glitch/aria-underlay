use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;

use crate::api::product_http::{
    ProductHttpErrorResponse, ProductHttpMethod, ProductHttpRequest, ProductHttpResponse,
    ProductHttpRouter,
};
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const HEADER_END: &[u8] = b"\r\n\r\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductHttpListenerConfig {
    pub bind_addr: SocketAddr,
    pub max_body_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct ProductHttpServer {
    router: ProductHttpRouter,
    config: ProductHttpListenerConfig,
}

impl Default for ProductHttpListenerConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 8088)),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }
}

impl ProductHttpListenerConfig {
    pub fn loopback(port: u16) -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], port)),
            ..Self::default()
        }
    }

    pub fn validate(&self) -> UnderlayResult<()> {
        if self.max_body_bytes == 0 {
            return Err(UnderlayError::InvalidIntent(
                "product HTTP max_body_bytes must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

impl ProductHttpServer {
    pub fn new(
        router: ProductHttpRouter,
        config: ProductHttpListenerConfig,
    ) -> UnderlayResult<Self> {
        config.validate()?;
        Ok(Self { router, config })
    }

    pub fn config(&self) -> ProductHttpListenerConfig {
        self.config
    }

    pub async fn serve_until_shutdown<S>(&self, shutdown: S) -> UnderlayResult<()>
    where
        S: Future<Output = ()> + Send,
    {
        let listener = TcpListener::bind(self.config.bind_addr)
            .await
            .map_err(product_http_io_error)?;
        self.serve_listener_until_shutdown(listener, shutdown).await
    }

    pub async fn serve_listener_until_shutdown<S>(
        &self,
        listener: TcpListener,
        shutdown: S,
    ) -> UnderlayResult<()>
    where
        S: Future<Output = ()> + Send,
    {
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    return Ok(());
                }
                accepted = listener.accept() => {
                    let (stream, peer_addr) = accepted.map_err(product_http_io_error)?;
                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(error) = server.serve_stream(stream).await {
                            eprintln!(
                                "ts={} level=warn component=product_http action=connection_failed peer_addr={} error={}",
                                now_unix_secs(),
                                peer_addr,
                                format_product_http_log_value(&error.to_string())
                            );
                        }
                    });
                }
            }
        }
    }

    pub fn handle_http_bytes(&self, payload: &[u8]) -> Vec<u8> {
        let response = match parse_http_request(payload, self.config.max_body_bytes) {
            Ok(request) => self.router.handle(request),
            Err(response) => response,
        };
        encode_http_response(response)
    }

    async fn serve_stream(&self, mut stream: TcpStream) -> UnderlayResult<()> {
        let request = read_http_request(&mut stream, self.config.max_body_bytes).await?;
        let response = self.handle_http_bytes(&request);
        stream
            .write_all(&response)
            .await
            .map_err(product_http_io_error)?;
        stream.shutdown().await.map_err(product_http_io_error)?;
        Ok(())
    }
}

async fn read_http_request(
    stream: &mut TcpStream,
    max_body_bytes: usize,
) -> UnderlayResult<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.map_err(product_http_io_error)?;
        if read == 0 {
            return Ok(buffer);
        }
        buffer.extend_from_slice(&chunk[..read]);

        let Some(header_end) = header_end_offset(&buffer) else {
            if buffer.len() > MAX_HEADER_BYTES {
                return Ok(buffer);
            }
            continue;
        };

        let headers = parse_header_block(&buffer[..header_end]).unwrap_or_default();
        let content_length = content_length(&headers).unwrap_or_default();
        if content_length > max_body_bytes {
            return Ok(buffer);
        }
        if buffer.len() >= header_end + HEADER_END.len() + content_length {
            return Ok(buffer);
        }
    }
}

fn parse_http_request(
    payload: &[u8],
    max_body_bytes: usize,
) -> Result<ProductHttpRequest, ProductHttpResponse> {
    let header_end = header_end_offset(payload).ok_or_else(|| {
        server_error_response(
            &BTreeMap::new(),
            400,
            "malformed_http",
            "product HTTP request is missing header terminator",
        )
    })?;
    if header_end > MAX_HEADER_BYTES {
        return Err(server_error_response(
            &BTreeMap::new(),
            400,
            "headers_too_large",
            "product HTTP headers exceed the configured safety limit",
        ));
    }

    let header_text = std::str::from_utf8(&payload[..header_end]).map_err(|err| {
        server_error_response(
            &BTreeMap::new(),
            400,
            "malformed_http",
            format!("product HTTP headers must be utf-8: {err}"),
        )
    })?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let (method, path) = parse_request_line(request_line)?;
    let headers = parse_headers(lines)?;

    if let Some(transfer_encoding) = header_value(&headers, "transfer-encoding") {
        if !transfer_encoding.eq_ignore_ascii_case("identity") {
            return Err(server_error_response(
                &headers,
                400,
                "unsupported_transfer_encoding",
                "product HTTP server requires Content-Length and does not accept transfer-encoding",
            ));
        }
    }

    let content_length = match content_length(&headers) {
        Ok(length) => length,
        Err(message) => {
            return Err(server_error_response(
                &headers,
                400,
                "invalid_content_length",
                message,
            ))
        }
    };
    if content_length > max_body_bytes {
        return Err(server_error_response(
            &headers,
            413,
            "payload_too_large",
            format!("product HTTP body exceeds {max_body_bytes} bytes"),
        ));
    }

    let body_start = header_end + HEADER_END.len();
    let body_end = body_start + content_length;
    if payload.len() < body_end {
        return Err(server_error_response(
            &headers,
            400,
            "incomplete_body",
            "product HTTP body is shorter than Content-Length",
        ));
    }

    Ok(ProductHttpRequest {
        method,
        path,
        headers,
        body: payload[body_start..body_end].to_vec(),
    })
}

fn parse_request_line(line: &str) -> Result<(ProductHttpMethod, String), ProductHttpResponse> {
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or_else(|| {
        server_error_response(
            &BTreeMap::new(),
            400,
            "malformed_http",
            "product HTTP request line is missing method",
        )
    })?;
    let target = parts.next().ok_or_else(|| {
        server_error_response(
            &BTreeMap::new(),
            400,
            "malformed_http",
            "product HTTP request line is missing target",
        )
    })?;
    let version = parts.next().ok_or_else(|| {
        server_error_response(
            &BTreeMap::new(),
            400,
            "malformed_http",
            "product HTTP request line is missing HTTP version",
        )
    })?;
    if parts.next().is_some() || version != "HTTP/1.1" {
        return Err(server_error_response(
            &BTreeMap::new(),
            400,
            "malformed_http",
            "product HTTP server accepts only HTTP/1.1 request lines",
        ));
    }

    let method = match method {
        "GET" => ProductHttpMethod::Get,
        "POST" => ProductHttpMethod::Post,
        "PUT" => ProductHttpMethod::Put,
        "PATCH" => ProductHttpMethod::Patch,
        "DELETE" => ProductHttpMethod::Delete,
        other => ProductHttpMethod::Other(other.into()),
    };
    let path = target.split('?').next().unwrap_or(target).to_string();
    Ok((method, path))
}

fn parse_header_block(payload: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let header_text =
        std::str::from_utf8(payload).map_err(|err| format!("invalid header utf-8: {err}"))?;
    let mut lines = header_text.split("\r\n");
    let _ = lines.next();
    parse_headers(lines).map_err(|response| {
        String::from_utf8(response.body)
            .unwrap_or_else(|_| "invalid product HTTP headers".into())
    })
}

fn parse_headers<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, String>, ProductHttpResponse> {
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(server_error_response(
                &headers,
                400,
                "malformed_http",
                format!("product HTTP header is missing ':' separator: {line}"),
            ));
        };
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err(server_error_response(
                &headers,
                400,
                "malformed_http",
                "product HTTP header name must not be empty",
            ));
        }
        headers.insert(name, value.trim().to_string());
    }
    Ok(headers)
}

fn content_length(headers: &BTreeMap<String, String>) -> Result<usize, String> {
    header_value(headers, "content-length")
        .map(|value| {
            value.parse::<usize>().map_err(|err| {
                format!("product HTTP Content-Length must be a non-negative integer: {err}")
            })
        })
        .transpose()
        .map(|length| length.unwrap_or(0))
}

fn encode_http_response(response: ProductHttpResponse) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(
        format!(
            "HTTP/1.1 {} {}\r\n",
            response.status,
            reason_phrase(response.status)
        )
        .as_bytes(),
    );

    let mut headers = response.headers;
    headers.insert("connection".into(), "close".into());
    headers.insert("content-length".into(), response.body.len().to_string());
    for (name, value) in headers {
        payload.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    payload.extend_from_slice(b"\r\n");
    payload.extend_from_slice(&response.body);
    payload
}

fn server_error_response(
    headers: &BTreeMap<String, String>,
    status: u16,
    error_code: &str,
    message: impl Into<String>,
) -> ProductHttpResponse {
    let request_id = header_value(headers, "x-aria-request-id");
    let trace_id = header_value(headers, "x-aria-trace-id").or_else(|| request_id.clone());
    let body = serde_json::to_vec(&ProductHttpErrorResponse {
        request_id,
        trace_id,
        error_code: error_code.into(),
        message: message.into(),
    })
    .unwrap_or_else(|_| {
        br#"{"request_id":null,"trace_id":null,"error_code":"internal_error","message":"failed to serialize product HTTP server error"}"#
            .to_vec()
    });
    let mut response_headers = BTreeMap::new();
    response_headers.insert("content-type".into(), "application/json".into());
    ProductHttpResponse {
        status,
        headers: response_headers,
        body,
    }
}

fn header_value(headers: &BTreeMap<String, String>, name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn header_end_offset(payload: &[u8]) -> Option<usize> {
    payload
        .windows(HEADER_END.len())
        .position(|window| window == HEADER_END)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "Unknown",
    }
}

fn product_http_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("product HTTP server io error: {err}"))
}

fn format_product_http_log_value(value: &str) -> String {
    if value.chars().all(is_unquoted_product_http_log_char) {
        value.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "\"<unprintable>\"".into())
    }
}

fn is_unquoted_product_http_log_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | ',' | '@')
}
