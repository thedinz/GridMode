//! Client for the Lemon Squeezy License API (activate / validate / deactivate).
//! These endpoints authenticate with the customer's license key alone, so the
//! app never carries a Lemon Squeezy API key.

use super::entitlement::{KeyStatus, LicenseFacts};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LicenseResponse {
    #[serde(default)]
    pub activated: Option<bool>,
    #[serde(default)]
    pub valid: Option<bool>,
    #[serde(default)]
    pub deactivated: Option<bool>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub license_key: Option<LicenseKeyInfo>,
    #[serde(default)]
    pub instance: Option<InstanceInfo>,
    #[serde(default)]
    pub meta: Option<MetaInfo>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LicenseKeyInfo {
    pub status: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InstanceInfo {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MetaInfo {
    pub store_id: u64,
    pub product_id: u64,
    pub variant_id: u64,
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub variant_name: Option<String>,
}

impl LicenseResponse {
    /// The store/product/variant/status facts, when the response includes them.
    pub fn facts(&self) -> Option<LicenseFacts> {
        let meta = self.meta.as_ref()?;
        Some(LicenseFacts {
            status: self
                .license_key
                .as_ref()
                .map(|key| KeyStatus::parse(&key.status))
                .unwrap_or(KeyStatus::Unknown),
            store_id: meta.store_id,
            product_id: meta.product_id,
            variant_id: meta.variant_id,
        })
    }

    fn error_text(&self) -> String {
        self.error.clone().unwrap_or_default().to_lowercase()
    }

    pub fn is_activation_limit(&self) -> bool {
        self.error_text().contains("activation limit")
    }

    pub fn is_instance_missing(&self) -> bool {
        self.error_text().contains("instance")
    }
}

#[derive(Debug)]
pub enum ApiError {
    /// Offline, timed out, or a Lemon Squeezy outage (5xx / rate limit). Never
    /// a reason to revoke a license.
    Unreachable(String),
    /// Lemon Squeezy answered and refused the request.
    Rejected(Box<LicenseResponse>),
}

pub struct LicenseApi {
    base_url: String,
    client: reqwest::Client,
}

impl LicenseApi {
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("GridMode/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }

    pub async fn activate(
        &self,
        license_key: &str,
        instance_name: &str,
    ) -> Result<LicenseResponse, ApiError> {
        self.post(
            "activate",
            &[
                ("license_key", license_key),
                ("instance_name", instance_name),
            ],
        )
        .await
        .and_then(|response| require(response.activated, response))
    }

    /// Validates a key, and this installation's activation when `instance_id` is given.
    pub async fn validate(
        &self,
        license_key: &str,
        instance_id: Option<&str>,
    ) -> Result<LicenseResponse, ApiError> {
        let mut form = vec![("license_key", license_key)];
        if let Some(instance_id) = instance_id {
            form.push(("instance_id", instance_id));
        }
        self.post("validate", &form)
            .await
            .and_then(|response| require(response.valid, response))
    }

    pub async fn deactivate(
        &self,
        license_key: &str,
        instance_id: &str,
    ) -> Result<LicenseResponse, ApiError> {
        self.post(
            "deactivate",
            &[("license_key", license_key), ("instance_id", instance_id)],
        )
        .await
        .and_then(|response| require(response.deactivated, response))
    }

    async fn post(
        &self,
        endpoint: &str,
        form: &[(&str, &str)],
    ) -> Result<LicenseResponse, ApiError> {
        let response = self
            .client
            .post(format!("{}/{endpoint}", self.base_url))
            .header(reqwest::header::ACCEPT, "application/json")
            .form(form)
            .send()
            .await
            .map_err(|error| ApiError::Unreachable(error.to_string()))?;

        let status = response.status();
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ApiError::Unreachable(format!(
                "Lemon Squeezy returned HTTP {status}"
            )));
        }
        let body = response
            .text()
            .await
            .map_err(|error| ApiError::Unreachable(error.to_string()))?;
        let parsed: LicenseResponse = serde_json::from_str(&body).map_err(|_| {
            ApiError::Unreachable(format!(
                "Unexpected response from Lemon Squeezy (HTTP {status})"
            ))
        })?;
        if status.is_success() {
            Ok(parsed)
        } else {
            Err(ApiError::Rejected(Box::new(parsed)))
        }
    }
}

fn require(flag: Option<bool>, response: LicenseResponse) -> Result<LicenseResponse, ApiError> {
    if flag == Some(true) {
        Ok(response)
    } else {
        Err(ApiError::Rejected(Box::new(response)))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A one-endpoint-at-a-time HTTP stub: each request pops the next canned
    /// (status, body) and records the request line and form body.
    pub struct StubServer {
        pub url: String,
        pub requests: Arc<Mutex<Vec<String>>>,
    }

    impl StubServer {
        pub fn start(responses: Vec<(u16, &'static str)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let recorded = requests.clone();
            std::thread::spawn(move || {
                for (status, body) in responses {
                    let Ok((mut stream, _)) = listener.accept() else {
                        return;
                    };
                    let mut buffer = [0_u8; 8192];
                    let mut request = Vec::new();
                    loop {
                        let read = stream.read(&mut buffer).unwrap_or(0);
                        request.extend_from_slice(&buffer[..read]);
                        let text = String::from_utf8_lossy(&request);
                        if let Some(split) = text.find("\r\n\r\n") {
                            let length = text
                                .lines()
                                .find_map(|line| {
                                    line.to_lowercase()
                                        .strip_prefix("content-length: ")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if request.len() >= split + 4 + length || read == 0 {
                                break;
                            }
                        } else if read == 0 {
                            break;
                        }
                    }
                    let text = String::from_utf8_lossy(&request).to_string();
                    let line = text.lines().next().unwrap_or_default().to_string();
                    let body_start = text
                        .find("\r\n\r\n")
                        .map(|index| index + 4)
                        .unwrap_or(text.len());
                    recorded
                        .lock()
                        .unwrap()
                        .push(format!("{line} {}", &text[body_start..]));
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes());
                }
            });
            Self { url, requests }
        }
    }

    pub const VALID_V1: &str = r#"{"valid":true,"error":null,"license_key":{"status":"active"},"instance":null,"meta":{"store_id":10,"product_id":20,"variant_id":111,"product_name":"GridMode","variant_name":"GridMode 1.x"}}"#;

    #[test]
    fn distinguishes_rejections_from_outages() {
        let server = StubServer::start(vec![
            (200, VALID_V1),
            (404, r#"{"valid":false,"error":"license_key not found."}"#),
            (503, "Service Unavailable"),
            (
                400,
                r#"{"activated":false,"error":"This license key has reached the activation limit."}"#,
            ),
        ]);
        let api = LicenseApi::new(&server.url);
        tauri::async_runtime::block_on(async {
            let ok = api.validate("KEY", None).await.unwrap();
            assert_eq!(ok.facts().unwrap().variant_id, 111);

            assert!(matches!(
                api.validate("BAD", None).await,
                Err(ApiError::Rejected(_))
            ));
            assert!(matches!(
                api.validate("KEY", None).await,
                Err(ApiError::Unreachable(_))
            ));
            match api.activate("KEY", "GridMode").await {
                Err(ApiError::Rejected(response)) => assert!(response.is_activation_limit()),
                other => panic!("expected activation limit, got {other:?}"),
            }
        });
        let requests = server.requests.lock().unwrap();
        assert!(
            requests[0].starts_with("POST /validate") && requests[0].contains("license_key=KEY")
        );
        assert!(requests[3].contains("instance_name=GridMode"));
    }

    #[test]
    fn unreachable_servers_are_not_rejections() {
        // Nothing listens on this port.
        let api = LicenseApi::new("http://127.0.0.1:9");
        let result = tauri::async_runtime::block_on(api.validate("KEY", None));
        assert!(matches!(result, Err(ApiError::Unreachable(_))));
    }
}
