use horizon_perf_share::share_saved;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

#[test]
fn sharing_retries_the_pin_without_reuploading_or_exposing_the_owner_token() {
    let dir = std::env::temp_dir().join(format!("horizon-perf-share-retry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("capture.hzperf");
    let bytes = b"\x1f\x8bcapture".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let endpoint = format!("{origin}/api/perf/captures");
    let link = format!("{origin}/perf/c/abcdefghij");
    let expected_link = link.clone();
    let server = std::thread::spawn(move || {
        for step in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut headers = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
                headers.push_str(&line.to_ascii_lowercase());
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let (status, reply) = if step == 0 {
                assert_eq!(request, "POST /api/perf/captures HTTP/1.1\r\n");
                assert_eq!(body, bytes);
                assert!(headers.contains("user-agent: horizon/0.1"));
                (
                    201,
                    serde_json::json!({"id":"abcdefghij","url":link,"delete_token":"owner-secret","shared":false,"expires_utc":"temporary","bytes":bytes.len(),"summary":{"kind":"client","seconds":1.0,"version":"test"}}),
                )
            } else {
                assert_eq!(
                    request,
                    "POST /api/perf/captures/abcdefghij/share HTTP/1.1\r\n"
                );
                assert!(headers.contains("authorization: bearer owner-secret"));
                if step == 1 {
                    (
                        503,
                        serde_json::json!({"error":{"code":"unavailable","message":"retry later"}}),
                    )
                } else {
                    (
                        200,
                        serde_json::json!({"id":"abcdefghij","url":link,"shared":true,"expires_utc":"pinned"}),
                    )
                }
            };
            let body = reply.to_string();
            write!(socket, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let error = share_saved(&endpoint, &path).unwrap_err();
    assert!(error.contains("retry later"));
    assert!(!error.contains("owner-secret"));
    let receipt = path.with_extension("hzperf.share.json");
    let owner: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(owner["upload"]["delete_token"], "owner-secret");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(receipt).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let reply = share_saved(&endpoint, &path).unwrap();
    assert_eq!(reply.url, expected_link);
    assert!(reply.shared);
    assert_eq!(reply.expires_utc, "pinned");
    server.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Answers one request per step with its status and JSON, checking each request line; returns the bearer tokens seen.
fn serve(listener: TcpListener, steps: Vec<(String, u16, serde_json::Value)>) -> Vec<String> {
    let mut tokens = Vec::new();
    for (expected, status, reply) in steps {
        let (mut socket, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&mut socket);
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().unwrap();
            }
            if let Some(token) = lower.strip_prefix("authorization: bearer ") {
                tokens.push(token.trim().to_owned());
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        assert_eq!(request, format!("{expected} HTTP/1.1\r\n"));
        let body = reply.to_string();
        write!(socket, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    }
    tokens
}

#[test]
fn an_upload_the_server_no_longer_has_is_made_again_and_its_receipt_replaced() {
    let dir = std::env::temp_dir().join(format!("horizon-perf-share-gone-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("capture.hzperf");
    std::fs::write(&path, b"\x1f\x8bcapture").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let endpoint = format!("{origin}/api/perf/captures");
    let receipt = path.with_extension("hzperf.share.json");
    let old = serde_json::json!({"endpoint": endpoint, "upload": {"id":"oldoldoldo","url":format!("{origin}/perf/c/oldoldoldo"),"delete_token":"old-secret","shared":true,"expires_utc":"long ago","bytes":9,"summary":{"kind":"client","seconds":1.0,"version":"test"}}});
    std::fs::write(&receipt, old.to_string()).unwrap();
    let link = format!("{origin}/perf/c/newnewnewn");
    let steps = vec![
        (
            "POST /api/perf/captures/oldoldoldo/share".to_owned(),
            404,
            serde_json::json!({"error":{"code":"not_found","message":"no such capture"}}),
        ),
        (
            "POST /api/perf/captures".to_owned(),
            201,
            serde_json::json!({"id":"newnewnewn","url":link,"delete_token":"new-secret","shared":false,"expires_utc":"temporary","bytes":9,"summary":{"kind":"client","seconds":1.0,"version":"test"}}),
        ),
        (
            "POST /api/perf/captures/newnewnewn/share".to_owned(),
            200,
            serde_json::json!({"id":"newnewnewn","url":link,"shared":true,"expires_utc":"pinned"}),
        ),
    ];
    let server = std::thread::spawn(move || serve(listener, steps));
    let reply = share_saved(&endpoint, &path).unwrap();
    assert_eq!(reply.url, link);
    assert_eq!(server.join().unwrap(), ["old-secret", "new-secret"]);
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(saved["upload"]["id"], "newnewnewn");
    assert_eq!(saved["upload"]["delete_token"], "new-secret");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&receipt).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
