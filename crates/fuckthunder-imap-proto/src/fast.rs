//! Allocation-free response scanning used by the benchmark and parity tools.
//!
//! The typed parser in [`crate::response`] is convenient but allocates a token
//! tree. This module provides a data-oriented fast path that only classifies
//! responses and counts their kinds. It is the kernel measured in the
//! `soa_simd` benchmark stage: `memchr` supplies the vectorised CRLF search and
//! no intermediate buffers are built.

use memchr::memchr;

/// Kind of a single response, as seen by the fast scanner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseKind {
    /// A tagged completion (`a1 OK ...`).
    Tagged,
    /// An untagged response (`* ...`).
    Untagged,
    /// A continuation request (`+ ...`).
    Continuation,
}

/// Aggregate counters produced by [`scan_corpus`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanSummary {
    /// Total responses scanned.
    pub responses: usize,
    /// Tagged responses.
    pub tagged: usize,
    /// Untagged responses.
    pub untagged: usize,
    /// Continuation requests.
    pub continuation: usize,
    /// `FETCH` responses.
    pub fetch: usize,
    /// `EXISTS` responses.
    pub exists: usize,
    /// `EXPUNGE` responses.
    pub expunge: usize,
    /// `LIST`/`LSUB` responses.
    pub list: usize,
    /// Total bytes consumed.
    pub bytes: usize,
}

impl ScanSummary {
    /// Folds another summary into this one.
    pub fn merge(&mut self, other: &Self) {
        self.responses += other.responses;
        self.tagged += other.tagged;
        self.untagged += other.untagged;
        self.continuation += other.continuation;
        self.fetch += other.fetch;
        self.exists += other.exists;
        self.expunge += other.expunge;
        self.list += other.list;
        self.bytes += other.bytes;
    }
}

/// Classifies one response without allocating.
#[must_use]
pub fn classify(input: &[u8]) -> Option<ResponseKind> {
    match input.first()? {
        b'*' => Some(ResponseKind::Untagged),
        b'+' => Some(ResponseKind::Continuation),
        _ => Some(ResponseKind::Tagged),
    }
}

/// Returns the end offset (exclusive) of the first complete response in `input`.
///
/// The scan understands synchronising and non-synchronising literals, so a
/// literal payload containing CRLF does not split the response.
#[must_use]
pub fn response_end(input: &[u8]) -> Option<usize> {
    let mut pos = 0;
    loop {
        let relative = memchr(b'\n', &input[pos..])?;
        let line_end = pos + relative;
        let line = &input[pos..line_end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        match literal_length(line) {
            Some(length) => {
                let data_start = line_end + 1;
                let data_end = data_start.checked_add(length)?;
                if data_end > input.len() {
                    return None;
                }
                pos = data_end;
            }
            None => return Some(line_end + 1),
        }
    }
}

/// Returns the literal length announced by a line ending in `{n}` / `{n+}`.
#[must_use]
pub fn literal_length(line: &[u8]) -> Option<usize> {
    let close = line.len().checked_sub(1)?;
    if line.get(close) != Some(&b'}') {
        return None;
    }
    let open = line[..close].iter().rposition(|b| *b == b'{')?;
    let digits = &line[open + 1..close];
    let digits = digits.strip_suffix(b"+").unwrap_or(digits);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// Splits a corpus into complete responses, borrowing from the input.
#[must_use]
pub fn split_responses(corpus: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = corpus;
    while !rest.is_empty() {
        match response_end(rest) {
            Some(end) if end > 0 => {
                out.push(&rest[..end]);
                rest = &rest[end..];
            }
            _ => break,
        }
    }
    out
}

/// Scans a whole corpus and counts response kinds.
///
/// # Examples
///
/// ```
/// use fuckthunder_imap_proto::fast::{scan_corpus, ResponseKind};
/// let corpus = b"* 1 EXISTS\r\n* 1 FETCH (UID 7)\r\na1 OK done\r\n";
/// let summary = scan_corpus(corpus);
/// assert_eq!(summary.responses, 3);
/// assert_eq!(summary.fetch, 1);
/// assert_eq!(summary.tagged, 1);
/// ```
#[must_use]
pub fn scan_corpus(corpus: &[u8]) -> ScanSummary {
    let mut summary = ScanSummary::default();
    for response in split_responses(corpus) {
        summary.responses += 1;
        summary.bytes += response.len();
        match classify(response) {
            Some(ResponseKind::Tagged) => summary.tagged += 1,
            Some(ResponseKind::Continuation) => summary.continuation += 1,
            Some(ResponseKind::Untagged) => {
                summary.untagged += 1;
                count_untagged(response, &mut summary);
            }
            None => {}
        }
    }
    summary
}

fn count_untagged(response: &[u8], summary: &mut ScanSummary) {
    let mut words = response.split(|b| matches!(b, b' ' | b'\r' | b'\n'));
    let _star = words.next();
    let mut second = words.next().unwrap_or_default();
    if second.first().is_some_and(u8::is_ascii_digit) {
        second = words.next().unwrap_or_default();
    }
    match second {
        b"FETCH" => summary.fetch += 1,
        b"EXISTS" => summary.exists += 1,
        b"EXPUNGE" => summary.expunge += 1,
        b"LIST" | b"LSUB" => summary.list += 1,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_aware_splitting() {
        let corpus = b"* 1 FETCH (BODY[] {5}\r\nh\r\nlo)\r\n* 2 EXISTS\r\n";
        let responses = split_responses(corpus);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0], b"* 1 FETCH (BODY[] {5}\r\nh\r\nlo)\r\n");
        assert_eq!(responses[1], b"* 2 EXISTS\r\n");
    }

    #[test]
    fn literal_plus_is_understood() {
        assert_eq!(literal_length(b"a1 APPEND INBOX {3+}"), Some(3));
        assert_eq!(literal_length(b"a1 APPEND INBOX {12}"), Some(12));
        assert_eq!(literal_length(b"a1 NOOP"), None);
        assert_eq!(literal_length(b"a1 APPEND {x}"), None);
    }

    #[test]
    fn summary_counts() {
        let corpus = b"* 1 EXISTS\r\n* 2 FETCH (UID 4)\r\n* 3 EXPUNGE\r\n* LIST () \"/\" INBOX\r\na1 OK done\r\n+ go\r\n";
        let summary = scan_corpus(corpus);
        assert_eq!(summary.responses, 6);
        assert_eq!(summary.tagged, 1);
        assert_eq!(summary.continuation, 1);
        assert_eq!(summary.untagged, 4);
        assert_eq!(summary.fetch, 1);
        assert_eq!(summary.exists, 1);
        assert_eq!(summary.expunge, 1);
        assert_eq!(summary.list, 1);
        assert_eq!(summary.bytes, corpus.len());
    }

    #[test]
    fn truncated_corpus_is_ignored() {
        assert!(response_end(b"* 1 FETCH {99}\r\nshort").is_none());
        assert_eq!(scan_corpus(b"* 1 EXISTS\r\npartial").responses, 1);
    }

    #[test]
    fn merge_adds_counters() {
        let mut a = scan_corpus(b"* 1 EXISTS\r\n");
        let b = scan_corpus(b"a1 OK\r\n");
        a.merge(&b);
        assert_eq!(a.responses, 2);
        assert_eq!(a.tagged, 1);
    }
}
