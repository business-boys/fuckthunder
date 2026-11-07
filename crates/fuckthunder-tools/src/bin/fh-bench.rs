//! `fh-bench` — measures typed parsing and the allocation-free scanner.

use std::path::PathBuf;
use std::time::Instant;

use fuckthunder_imap_proto::fast::{scan_corpus, split_responses};
use fuckthunder_imap_proto::parse_response;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut corpus_path = None;
    let mut iterations = 20usize;
    let mut mode = "typed".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--corpus" => corpus_path = args.next().map(PathBuf::from),
            "--iterations" => {
                iterations = args.next().and_then(|v| v.parse().ok()).unwrap_or(20);
            }
            "--mode" => mode = args.next().unwrap_or_else(|| "typed".into()),
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(corpus_path) = corpus_path else {
        eprintln!("usage: fh-bench --corpus corpus.imap [--iterations N] [--mode typed|fast]");
        std::process::exit(2);
    };
    let corpus = std::fs::read(&corpus_path).expect("read corpus");
    let responses = split_responses(&corpus).len();

    let start = Instant::now();
    let mut checksum = 0usize;
    for _ in 0..iterations {
        match mode.as_str() {
            "fast" => {
                checksum = checksum.wrapping_add(scan_corpus(&corpus).responses);
            }
            _ => {
                for response in split_responses(&corpus) {
                    let parsed = parse_response(response).expect("parse");
                    checksum = checksum.wrapping_add(format!("{parsed:?}").len());
                }
            }
        }
    }
    let elapsed = start.elapsed();
    let total_bytes = corpus.len() * iterations;
    let millis = elapsed.as_secs_f64() * 1000.0;
    let mbps = if millis > 0.0 {
        (total_bytes as f64 / 1_048_576.0) / (millis / 1000.0)
    } else {
        0.0
    };
    let result = serde_json::json!({
        "mode": mode,
        "iterations": iterations,
        "responses_per_pass": responses,
        "corpus_bytes": corpus.len(),
        "total_bytes": total_bytes,
        "elapsed_ms": millis,
        "throughput_mb_s": mbps,
        "checksum": checksum,
    });
    println!("{result}");
}
