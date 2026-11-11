use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn sample() -> Metadata {
    Metadata {
        name: "Caches".into(),
        path: "~/Library/Caches".into(),
        kind: "directory".into(),
        logical_bytes: Some("4096".into()),
        allocated_bytes: None,
        partial: true,
        excluded: false,
    }
}

#[test]
fn previews_are_independent_bounded_expiring_and_single_use() {
    let state = AiState::default();
    let first = state.prepare(sample()).unwrap();
    let second = state.prepare(sample()).unwrap();
    assert_eq!(
        state.take("not-an-id").err().unwrap().code,
        "ai_preview_expired"
    );
    assert_eq!(
        state.take(&first).unwrap().metadata.path,
        "~/Library/Caches"
    );
    assert_eq!(state.take(&first).err().unwrap().code, "ai_preview_expired");
    state
        .pending
        .lock()
        .unwrap()
        .get_mut(&second)
        .unwrap()
        .created = Instant::now() - Duration::from_secs(601);
    assert_eq!(
        state.take(&second).err().unwrap().code,
        "ai_preview_expired"
    );
    for _ in 0..20 {
        state.prepare(sample()).unwrap();
    }
    assert_eq!(state.pending.lock().unwrap().len(), 8);
}

#[test]
fn key_validation_never_echoes_the_input() {
    assert_eq!(
        validate_key("  sk-fixture-only-123456  ").unwrap(),
        "sk-fixture-only-123456"
    );
    for key in ["", "short", "sk-private-secret\nheader: malicious"] {
        let error = validate_key(key).unwrap_err();
        assert_eq!(error.code, "invalid_api_key");
        assert!(!error.message.contains("private-secret"));
    }
}

#[test]
fn metadata_redacts_home_and_preserves_unknown() {
    let entry = Entry {
        id: "1".into(),
        parent_id: None,
        name: "Caches".into(),
        path_display: "/Users/person/Library/Caches".into(),
        kind: "directory".into(),
        logical: None,
        allocated: None,
        known_logical: "0".into(),
        known_allocated: "0".into(),
        unknown_logical: "1".into(),
        unknown_allocated: "1".into(),
        gaps: "1".into(),
        observed_logical: "0".into(),
        observed_allocated: "0".into(),
        files: "0".into(),
        canonical_id: None,
        excluded: false,
        note: None,
        modified_at: None,
    };
    let data = metadata(&entry, Path::new("/Users/person"));
    assert_eq!(data.path, "~/Library/Caches");
    assert!(data.logical_bytes.is_none());
    assert!(data.allocated_bytes.is_none());
    assert!(data.partial);
    let root = metadata(
        &Entry {
            path_display: "/Users/person".into(),
            name: "person".into(),
            ..entry.clone()
        },
        Path::new("/Users/person"),
    );
    assert_eq!(root.name, "~");
    assert_eq!(root.path, "~");
    let other = metadata(
        &Entry {
            path_display: "/Users/person2/data".into(),
            ..entry
        },
        Path::new("/Users/person"),
    );
    assert_eq!(other.path, "/Users/person2/data");
    let prompt = payload(&data);
    let user = prompt["messages"][1]["content"].as_str().unwrap();
    assert!(!user.contains("/Users/person"));
    assert!(serde_json::from_str::<serde_json::Value>(user).is_ok());
    assert_eq!(prompt["thinking"]["type"], "disabled");
    assert_eq!(prompt["model"], MODEL);
}

#[test]
fn empty_invalid_and_truncated_answers_are_distinct() {
    assert_eq!(decode(b"{}").unwrap_err().code, "ai_invalid_response");
    assert_eq!(
        decode(br#"{"choices":[]}"#).unwrap_err().code,
        "ai_empty_response"
    );
    assert_eq!(
        decode(br#"{"choices":[{"message":{"content":"  "}}]}"#)
            .unwrap_err()
            .code,
        "ai_empty_response"
    );
    let answer=decode(br#"{"choices":[{"message":{"content":"<script>not executable</script>"},"finish_reason":"length"}]}"#).unwrap();
    assert!(answer.truncated);
    assert!(answer.text.contains("<script>")); // UI must render this as text, not HTML.
}

fn server(status: u16, body: String, delay: Duration) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let size = stream.read(&mut buffer).unwrap();
            if size == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..size]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        std::thread::sleep(delay);
        let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:1/never-follow\r\nConnection: close\r\n\r\n{body}",body.len());
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8(request).unwrap()
    });
    (format!("http://{address}/chat/completions"), thread)
}
fn test_client(timeout: Duration) -> Client {
    Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .timeout(timeout)
        .build()
        .unwrap()
}

#[tokio::test]
async fn sends_only_preview_metadata_and_reads_answer() {
    let body=serde_json::json!({"choices":[{"message":{"content":"通常用于保存应用缓存。"},"finish_reason":"stop"}]}).to_string();
    let (url, handle) = server(200, body, Duration::ZERO);
    let result = request(
        &test_client(Duration::from_secs(2)),
        &url,
        "sk-fixture-only-123456",
        &sample(),
    )
    .await
    .unwrap();
    assert_eq!(result.text, "通常用于保存应用缓存。");
    let sent = handle.join().unwrap();
    assert!(sent
        .to_ascii_lowercase()
        .contains("authorization: bearer sk-fixture-only-123456"));
    let body: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body, payload(&sample()));
    assert!(!result.truncated);
}

#[tokio::test]
async fn maps_api_failures_without_exposing_response_bodies_or_following_redirects() {
    for (status, code) in [
        (401, "ai_auth_failed"),
        (402, "ai_balance_insufficient"),
        (429, "ai_rate_limited"),
        (503, "ai_service_unavailable"),
        (302, "ai_request_rejected"),
    ] {
        let (url, handle) = server(
            status,
            "private provider diagnostics".into(),
            Duration::ZERO,
        );
        let error = request(
            &test_client(Duration::from_secs(2)),
            &url,
            "sk-fixture-only-123456",
            &sample(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!error.message.contains("private"));
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn bounds_response_size_and_wait_time() {
    let (url, handle) = server(200, "x".repeat(RESPONSE_LIMIT + 1), Duration::ZERO);
    assert_eq!(
        request(
            &test_client(Duration::from_secs(2)),
            &url,
            "sk-fixture-only-123456",
            &sample()
        )
        .await
        .unwrap_err()
        .code,
        "ai_response_too_large"
    );
    handle.join().unwrap();
    let (url, handle) = server(200, "{}".into(), Duration::from_millis(300));
    assert_eq!(
        request(
            &test_client(Duration::from_millis(100)),
            &url,
            "sk-fixture-only-123456",
            &sample()
        )
        .await
        .unwrap_err()
        .code,
        "ai_timeout"
    );
    handle.join().unwrap();
    assert!(http_client(Duration::from_secs(1))
        .unwrap()
        .get("http://api.deepseek.com/chat/completions")
        .send()
        .await
        .is_err());
}
