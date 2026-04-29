//! Minimal bearer-token authentication for the HTTP transport.

use axum::http::StatusCode;
use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};

#[derive(Clone, Debug)]
pub struct TokenState {
    pub expected: Option<String>,
}

pub async fn bearer_auth(
    State(state): State<TokenState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = state.expected.as_deref() else {
        return Ok(next.run(req).await);
    };
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header.strip_prefix("Bearer ").unwrap_or("");
    if constant_time_eq(token.as_bytes(), expected.as_bytes()) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_same() {
        assert!(constant_time_eq(b"abcd", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abce"));
        assert!(!constant_time_eq(b"abcd", b"abc"));
    }
}
