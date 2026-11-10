mod common;

use horizon_perf::sharing::share_saved;
use horizon_perf_format::v1::Kind;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

#[test]
fn sharing_retries_the_pin_without_reuploading_or_exposing_the_owner_token() {
    let dir = common::TempDir::new("share-retry");
    let path = dir.path().join("capture.hzperf");
    let bytes = common::write_capture(&[
        common::start(common::CLIENT, Kind::Client),
        common::stop(common::CLIENT, 1.0),
    ])
    .bytes;
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
}
