//! Smoke tests for the `fuckthunder` CLI against a mock server.

use std::io::Read;
use std::process::{Command, Stdio};

#[test]
fn help_and_usage_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_fuckthunder"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("IMAP + OAuth"));

    let output = Command::new(env!("CARGO_BIN_EXE_fuckthunder"))
        .arg("--definitely-not-a-flag")
        .output()
        .unwrap();
    assert!(!output.status.success());

    // Every subcommand parses.
    for sub in ["capability", "login", "list", "select", "fetch", "search"] {
        let output = Command::new(env!("CARGO_BIN_EXE_fuckthunder"))
            .args([sub, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{sub} --help failed");
    }
}

#[test]
fn capability_against_mock_server() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let port = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut io = BufReader::new(socket);
            io.write_all(b"* OK [CAPABILITY IMAP4rev1 IDLE] mock\r\n")
                .await
                .unwrap();
            let mut line = Vec::new();
            io.read_until(b'\n', &mut line).await.unwrap();
            let tag = String::from_utf8_lossy(&line)
                .split_whitespace()
                .next()
                .unwrap_or("a1")
                .to_owned();
            io.write_all(b"* CAPABILITY IMAP4rev1 IDLE\r\n")
                .await
                .unwrap();
            io.write_all(format!("{tag} OK done\r\n").as_bytes())
                .await
                .unwrap();
        });
        port
    });

    let output = Command::new(env!("CARGO_BIN_EXE_fuckthunder"))
        .args(["--tls", "plain", "--port", &port.to_string(), "capability"])
        .env("FUCKTHUNDER_HOST", "127.0.0.1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("IMAP4REV1"), "{stdout}");
}

#[test]
fn invalid_sequence_set_is_rejected() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fuckthunder"))
        .args(["--tls", "plain", "fetch", "not-a-set"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    let mut stderr = String::new();
    child.stderr.unwrap().read_to_string(&mut stderr).ok();
}
