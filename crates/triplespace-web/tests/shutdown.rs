//! The web tier's shutdown (0057 §11; 22 §4.7): on `SIGTERM` it stops accepting, finishes
//! what is in flight, and exits cleanly, so an orchestrator can remove a replica.

#![cfg(unix)]

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A port nothing listens on now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

// 0057 §11: SIGTERM is a clean stop, as an interrupt is.
#[test]
fn sigterm_stops_the_web_tier_cleanly() {
    let port = free_port();
    // The API need not answer: the tier connects to it only to draw a page.
    let mut child = Command::new(env!("CARGO_BIN_EXE_triplespace-web"))
        .args([
            "--api",
            "http://127.0.0.1:9",
            "--listen",
            &format!("127.0.0.1:{port}"),
        ])
        .env_remove("TRIPLESPACE_ENV_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the web tier starts");
    // It says where it listens once it does.
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let listening = lines
        .by_ref()
        .map_while(Result::ok)
        .any(|l| l.contains(&format!("127.0.0.1:{port}")));
    assert!(listening, "the web tier did not start listening");

    // It serves: an asset path needs nothing of the API.
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("it accepts");
    stream
        .write_all(
            b"GET /ui/assets/none.css HTTP/1.1\r\nHost: wiki.example\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 404"), "{answer}");

    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success());
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("the web tier was still running ten seconds after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        status.success(),
        "it exits cleanly, not by the signal: {status}"
    );
    // And it no longer accepts.
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}
