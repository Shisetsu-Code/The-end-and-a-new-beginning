// Synthetic local responses; no provider sessions or remote requests.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct RecordedCommand {
    target: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

async fn command_fixture(
    provider: &str,
) -> (
    tempfile::TempDir,
    RunContext,
    String,
    tokio::task::JoinHandle<Vec<RecordedCommand>>,
) {
    let root = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let journal =
        crate::journal::Journal::create(&root.path().join("attempt"), &json!({})).unwrap();
    let mut ctx =
        RunContext::new(provider, journal, std::time::Duration::from_secs(2), 10).unwrap();
    ctx.test_origin = Some(origin.clone());
    let server = tokio::spawn(async move {
        let mut recorded = Vec::new();
        for index in 0..5 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut chunk = [0; 4096];
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0, "request ended before headers");
                bytes.extend_from_slice(&chunk[..read]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = std::str::from_utf8(&bytes[..end]).unwrap();
                    let length = header
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .unwrap()
                        .1
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let mut chunk = [0; 4096];
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0, "request ended before its body");
                bytes.extend_from_slice(&chunk[..read]);
            }
            let header = std::str::from_utf8(&bytes[..header_end]).unwrap();
            let mut lines = header.lines();
            let first = lines.next().unwrap();
            assert!(first.starts_with("POST "));
            let target = first.split_whitespace().nth(1).unwrap().to_owned();
            let headers = lines
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
                .collect();
            let body = bytes[header_end..header_end + length].to_vec();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let response = json!({
                "command": if index == 4 { json!("wrong_command") } else { request["command"].clone() },
                "request_id": if index == 3 { json!("wrong_request") } else { request["request_id"].clone() },
                "status": {"code":"OK"}, "session_id":"fixture-session",
                "user":{"huid":"fixture-huid"}, "context":{"round_finished":true}
            }).to_string();
            socket.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(), response
            ).as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
            recorded.push(RecordedCommand {
                target,
                headers,
                body,
            });
        }
        recorded
    });
    (
        root,
        ctx,
        format!("{origin}/betman-demo/gs/fixture/desktop/queue/demo/"),
        server,
    )
}

fn assert_command_contract(ctx: &RunContext, captured: &[RecordedCommand]) {
    assert_eq!(captured.len(), 5);
    let mut ids = std::collections::BTreeSet::new();
    for (request, command) in captured
        .iter()
        .zip(["login", "start", "play", "play", "play"])
    {
        assert_eq!(
            request.target,
            format!("/betman-demo/gs/fixture/desktop/queue/demo/?gsc={command}")
        );
        assert_eq!(request.headers["content-type"], "text/plain");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["command"], command);
        let id = body["request_id"].as_str().unwrap();
        assert_eq!(id.len(), 32);
        assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 4);
        assert!(
            ids.insert(id.to_owned()),
            "a new command must use a fresh request id"
        );
        assert!(body["client_command_timestamp"].as_u64().is_some());
        if command == "login" {
            assert_eq!(body["token"], "fixture-demo-token");
            assert_eq!(body["language"], "en");
        } else {
            assert_eq!(body["session_id"], "fixture-session");
        }
    }
    let journal = std::fs::read_to_string(ctx.journal.path().join("wire.jsonl")).unwrap();
    let records = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|row| row["event"] == "http_request")
        .collect::<Vec<_>>();
    assert_eq!(records.len(), captured.len());
    for (index, (row, request)) in records.iter().zip(captured).enumerate() {
        assert_eq!(
            STANDARD
                .decode(row["data"]["body_base64"].as_str().unwrap())
                .unwrap(),
            request.body
        );
        assert!(
            row["data"]["url"]
                .as_str()
                .unwrap()
                .ends_with(&request.target)
        );
        assert_eq!(row["data"]["round_action"], index >= 2);
    }
}
