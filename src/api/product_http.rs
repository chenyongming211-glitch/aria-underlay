use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::api::operations::ListOperationSummariesRequest;
use crate::api::product_api::{ProductApiRequest, ProductOpsApi};
use crate::api::product_ops::ExportProductAuditRequest;
use crate::UnderlayError;

pub const OPERATION_SUMMARIES_QUERY_PATH: &str = "/product/v1/operations/summaries:query";
pub const PRODUCT_AUDIT_EXPORT_PATH: &str = "/product/v1/product-audit:export";

const REQUEST_ID_HEADER: &str = "x-aria-request-id";
const TRACE_ID_HEADER: &str = "x-aria-trace-id";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductHttpRequest {
    pub method: ProductHttpMethod,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductHttpErrorResponse {
    pub request_id: Option<String>,
    pub trace_id: Option<String>,
    pub error_code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProductHttpRouter {
    api: ProductOpsApi,
}

impl ProductHttpRouter {
    pub fn new(api: ProductOpsApi) -> Self {
        Self { api }
    }

    pub fn handle(&self, request: ProductHttpRequest) -> ProductHttpResponse {
        let method = request.method.clone();
        let path = request.path.clone();
        match (method, path.as_str()) {
            (ProductHttpMethod::Post, OPERATION_SUMMARIES_QUERY_PATH) => {
                self.handle_operation_summaries_query(request)
            }
            (ProductHttpMethod::Post, PRODUCT_AUDIT_EXPORT_PATH) => {
                self.handle_product_audit_export(request)
            }
            (_, OPERATION_SUMMARIES_QUERY_PATH | PRODUCT_AUDIT_EXPORT_PATH) => {
                method_not_allowed_response(&request, "POST")
            }
            _ => not_found_response(&request),
        }
    }

    fn handle_operation_summaries_query(
        &self,
        request: ProductHttpRequest,
    ) -> ProductHttpResponse {
        match decode_body::<ListOperationSummariesRequest>(&request)
            .and_then(|body| product_api_request(&request, body))
            .and_then(|api_request| self.api.list_operation_summaries(api_request))
        {
            Ok(response) => json_response(200, &response),
            Err(error) => underlay_error_response(&request, error),
        }
    }

    fn handle_product_audit_export(&self, request: ProductHttpRequest) -> ProductHttpResponse {
        match decode_body::<ExportProductAuditRequest>(&request)
            .and_then(|body| product_api_request(&request, body))
            .and_then(|api_request| self.api.export_product_audit(api_request))
        {
            Ok(response) => json_response(200, &response),
            Err(error) => underlay_error_response(&request, error),
        }
    }
}

fn product_api_request<T>(
    request: &ProductHttpRequest,
    body: T,
) -> Result<ProductApiRequest<T>, UnderlayError> {
    Ok(ProductApiRequest {
        request_id: required_header(&request.headers, REQUEST_ID_HEADER)?,
        trace_id: optional_header(&request.headers, TRACE_ID_HEADER),
        headers: request.headers.clone(),
        body,
    })
}

fn decode_body<T>(request: &ProductHttpRequest) -> Result<T, UnderlayError>
where
    T: DeserializeOwned + Default,
{
    if request.body.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice(&request.body).map_err(|err| {
        UnderlayError::InvalidIntent(format!(
            "invalid JSON body for {}: {err}",
            request.path
        ))
    })
}

fn underlay_error_response(
    request: &ProductHttpRequest,
    error: UnderlayError,
) -> ProductHttpResponse {
    let (status, error_code) = match &error {
        UnderlayError::AuthenticationFailed(_) => (401, "authentication_failed"),
        UnderlayError::InvalidIntent(_) => (400, "invalid_request"),
        UnderlayError::AuthorizationDenied(_) => (403, "authorization_denied"),
        UnderlayError::DeviceNotFound(_) => (404, "not_found"),
        UnderlayError::ProductAuditWriteFailed(_) => (500, "product_audit_write_failed"),
        UnderlayError::AdapterTransport(_) | UnderlayError::AdapterOperation { .. } => {
            (502, "adapter_error")
        }
        _ => (500, "internal_error"),
    };
    let mut response = error_response(request, status, error_code, error.to_string());
    if status == 401 {
        response
            .headers
            .insert("www-authenticate".into(), "Bearer".into());
    }
    response
}

fn not_found_response(request: &ProductHttpRequest) -> ProductHttpResponse {
    error_response(
        request,
        404,
        "not_found",
        format!("unknown product HTTP path {}", request.path),
    )
}

fn method_not_allowed_response(request: &ProductHttpRequest, allow: &str) -> ProductHttpResponse {
    let mut response = error_response(
        request,
        405,
        "method_not_allowed",
        format!("method {:?} is not allowed for {}", request.method, request.path),
    );
    response.headers.insert("allow".into(), allow.into());
    response
}

fn error_response(
    request: &ProductHttpRequest,
    status: u16,
    error_code: &str,
    message: String,
) -> ProductHttpResponse {
    json_response(
        status,
        &ProductHttpErrorResponse {
            request_id: optional_header(&request.headers, REQUEST_ID_HEADER),
            trace_id: trace_id_for_error(&request.headers),
            error_code: error_code.into(),
            message,
        },
    )
}

fn json_response<T: Serialize>(status: u16, body: &T) -> ProductHttpResponse {
    let mut headers = BTreeMap::new();
    headers.insert("content-type".into(), "application/json".into());
    let body = serde_json::to_vec(body).unwrap_or_else(|_| {
        br#"{"request_id":null,"trace_id":null,"error_code":"internal_error","message":"failed to serialize product HTTP response"}"#
            .to_vec()
    });
    ProductHttpResponse {
        status,
        headers,
        body,
    }
}

fn trace_id_for_error(headers: &BTreeMap<String, String>) -> Option<String> {
    optional_header(headers, TRACE_ID_HEADER).or_else(|| optional_header(headers, REQUEST_ID_HEADER))
}

fn required_header(headers: &BTreeMap<String, String>, name: &str) -> Result<String, UnderlayError> {
    optional_header(headers, name).ok_or_else(|| {
        UnderlayError::InvalidIntent(format!(
            "missing required product HTTP header {name}"
        ))
    })
}

fn optional_header(headers: &BTreeMap<String, String>, name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
