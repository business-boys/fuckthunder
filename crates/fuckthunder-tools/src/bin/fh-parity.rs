//! `fh-parity` — parses a corpus and emits fingerprints as JSON.

use std::path::PathBuf;

use fuckthunder_imap_proto::fast::split_responses;
use fuckthunder_imap_proto::parse_response;
use fuckthunder_tools::fingerprint;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut input = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input = args.next().map(PathBuf::from),
            "--out" => output = args.next().map(PathBuf::from),
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(input) = input else {
        eprintln!("usage: fh-parity --input corpus.imap [--out fingerprints.json]");
        std::process::exit(2);
    };
    let corpus = std::fs::read(&input).expect("read corpus");
    let mut prints = Vec::new();
    for (index, response) in split_responses(&corpus).into_iter().enumerate() {
        match parse_response(response) {
            Ok(parsed) => prints.push(fingerprint(&parsed)),
            Err(error) => {
                eprintln!("response {index} failed to parse: {error}");
                std::process::exit(1);
            }
        }
    }
    let json = serde_json::to_string_pretty(&prints).expect("serialise fingerprints");
    match output {
        Some(path) => std::fs::write(path, json).expect("write output"),
        None => println!("{json}"),
    }
}
