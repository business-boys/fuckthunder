//! Smoke tests for the `fh-parity` and `fh-bench` binaries.

use std::io::Write;
use std::process::Command;

fn corpus(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("corpus.imap");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(
        b"* OK ready\r\n* 2 EXISTS\r\n* 1 FETCH (UID 9 BODY[] {5}\r\nhello)\r\na1 OK done\r\n",
    )
    .unwrap();
    path
}

#[test]
fn fh_parity_fingerprints_a_corpus() {
    let dir = std::env::temp_dir().join(format!("fh-parity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let corpus = corpus(&dir);
    let out = dir.join("out.json");
    let status = Command::new(env!("CARGO_BIN_EXE_fh-parity"))
        .arg("--input")
        .arg(&corpus)
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 4);

    // Corpus to stdout when --out is omitted.
    let output = Command::new(env!("CARGO_BIN_EXE_fh-parity"))
        .arg("--input")
        .arg(&corpus)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"EXISTS\""));

    // Usage error on missing arguments.
    let output = Command::new(env!("CARGO_BIN_EXE_fh-parity"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn fh_bench_measures_both_modes() {
    let dir = std::env::temp_dir().join(format!("fh-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let corpus = corpus(&dir);
    for mode in ["typed", "fast"] {
        let output = Command::new(env!("CARGO_BIN_EXE_fh-bench"))
            .arg("--corpus")
            .arg(&corpus)
            .arg("--iterations")
            .arg("3")
            .arg("--mode")
            .arg(mode)
            .output()
            .unwrap();
        assert!(output.status.success(), "mode {mode}: {:?}", output);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(mode), "{stdout}");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fh-bench"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
