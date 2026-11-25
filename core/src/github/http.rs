use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HttpError(pub String);

pub trait Http: Send + Sync {
    fn send(&self, request: &Request) -> Result<Response, HttpError>;
}

const USER_AGENT: &str = "diffpilot";
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent }
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Http for UreqHttp {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        let method = match request.method {
            Method::Get => ureq::http::Method::GET,
            Method::Post => ureq::http::Method::POST,
        };
        let mut builder = ureq::http::Request::builder().method(method).uri(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let mut response = match &request.body {
            Some(body) => self.agent.run(builder.body(body.clone()).map_err(error)?),
            None => self.agent.run(builder.body(()).map_err(error)?),
        }
        .map_err(error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_string()
            .map_err(error)?;
        Ok(Response { status, body })
    }
}

fn error(error: impl std::fmt::Display) -> HttpError {
    HttpError(error.to_string())
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    pub struct FakeHttp {
        replies: Mutex<VecDeque<(String, Response)>>,
        pub seen: Mutex<Vec<Request>>,
    }

    impl FakeHttp {
        pub fn new() -> Self {
            Self {
                replies: Mutex::default(),
                seen: Mutex::default(),
            }
        }

        /// The next request must contain `needle` in its URL or body.
        pub fn reply(self, needle: &str, status: u16, body: &str) -> Self {
            self.replies.lock().unwrap().push_back((
                needle.to_string(),
                Response {
                    status,
                    body: body.to_string(),
                },
            ));
            self
        }

        pub fn requests(&self) -> Vec<Request> {
            self.seen.lock().unwrap().clone()
        }

        pub fn assert_done(&self) {
            assert!(self.replies.lock().unwrap().is_empty(), "unused fixtures left");
        }
    }

    impl Http for FakeHttp {
        fn send(&self, request: &Request) -> Result<Response, HttpError> {
            self.seen.lock().unwrap().push(request.clone());
            let (needle, response) = self.replies.lock().unwrap().pop_front().unwrap_or_else(|| {
                panic!(
                    "unexpected request {} {}",
                    request.url,
                    request.body.clone().unwrap_or_default()
                )
            });
            let haystack = format!("{} {}", request.url, request.body.clone().unwrap_or_default());
            assert!(
                haystack.contains(&needle),
                "request `{haystack}` does not contain `{needle}`"
            );
            Ok(response)
        }
    }
}
