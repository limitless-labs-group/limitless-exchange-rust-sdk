//! Minimal HTTP stub used by unit tests that exercise full request flows.

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

/// A request captured by [`spawn_routes`].
#[derive(Debug)]
pub(crate) struct CapturedRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    pub headers: std::collections::HashMap<String, String>,
}

impl CapturedRequest {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("captured request body should be JSON")
    }
}

/// Serves `routes` (`(method, path, status, body)`) on a local port until the
/// test ends and forwards every request it receives. Unknown routes get a 404.
pub(crate) fn spawn_routes(
    routes: Vec<(&'static str, &'static str, u16, String)>,
) -> (String, mpsc::Receiver<CapturedRequest>) {
    spawn_stub(routes, false)
}

/// Serve each matching response once, allowing state transitions on one route.
pub(crate) fn spawn_sequence(
    routes: Vec<(&'static str, &'static str, u16, String)>,
) -> (String, mpsc::Receiver<CapturedRequest>) {
    spawn_stub(routes, true)
}

fn spawn_stub(
    mut routes: Vec<(&'static str, &'static str, u16, String)>,
    consume: bool,
) -> (String, mpsc::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("address");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            let (head, body) = read_request(&mut stream);
            let mut request_line = head.lines().next().unwrap_or_default().split_whitespace();
            let method = request_line.next().unwrap_or_default().to_string();
            let path = request_line.next().unwrap_or_default().to_string();
            let matched = routes.iter().position(|(route_method, route_path, _, _)| {
                *route_method == method && path.split('?').next() == Some(*route_path)
            });
            let (status, response_body) = if let Some(index) = matched {
                let (_, _, status, body) = routes[index].clone();
                if consume {
                    routes.remove(index);
                }
                (status, body)
            } else {
                (404, r#"{"message":"not found"}"#.to_string())
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Stub\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            );
            let headers = head
                .lines()
                .skip(1)
                .filter_map(|line| line.split_once(':'))
                .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
                .collect();
            if sender
                .send(CapturedRequest {
                    method,
                    path,
                    body,
                    headers,
                })
                .is_err()
            {
                return;
            }
        }
    });
    (format!("http://{address}"), receiver)
}

fn read_request(stream: &mut impl Read) -> (String, String) {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).expect("request read");
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&bytes);
        let Some(header_end) = text.find("\r\n\r\n") else {
            continue;
        };
        let content_length = text[..header_end]
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
            .unwrap_or(0);
        if bytes.len() >= header_end + 4 + content_length {
            let text = String::from_utf8(bytes).expect("utf8 request");
            let (head, body) = text.split_at(header_end);
            return (head.to_string(), body[4..].to_string());
        }
    }
    (String::from_utf8_lossy(&bytes).into_owned(), String::new())
}
