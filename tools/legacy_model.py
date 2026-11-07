#!/usr/bin/env python3
"""Deterministic legacy-derived IMAP reference model.

This is the A-side of the differential parity harness.  It generates a corpus
of server responses from the grammar exercised by the Rust port and parses the
same bytes back into the normalised fingerprint shape used by ``fh-parity``.

The model is intentionally small and deterministic: the goal is to pin the
observable grammar (response kind, keyword, code, sequence/UID, counts), not to
reimplement Thunderbird's full XPCOM stack.
"""

from __future__ import annotations

import argparse
import base64
import json
import random
import re
from pathlib import Path

MAILBOXES = [
    "INBOX",
    "Sent Items",
    "&U,BTFw-",          # 台北
    "Archive/2026",
    "A&-B",              # A&B
    "Drafts",
]

CODES = ["READ-WRITE", "READ-ONLY", "TRYCREATE", "UNAVAILABLE", "UIDNEXT 7"]


def decode_mutf7(text: str) -> str:
    """Decodes IMAP modified UTF-7 (``/`` -> ``,``, ``+`` preserved)."""
    out: list[str] = []
    index = 0
    while index < len(text):
        char = text[index]
        if char != "&":
            out.append(char)
            index += 1
            continue
        end = text.find("-", index + 1)
        if end < 0:
            raise ValueError("unterminated modified UTF-7 run")
        run = text[index + 1 : end]
        if run == "":
            out.append("&")
        else:
            b64 = run.replace(",", "/")
            b64 += "=" * ((4 - len(b64) % 4) % 4)
            out.append(base64.b64decode(b64).decode("utf-16-be"))
        index = end + 1
    return "".join(out)


def unquote(value: str) -> str:
    """Unquotes an IMAP quoted string (the generator never emits NIL names)."""
    if len(value) >= 2 and value[0] == '"' and value[-1] == '"':
        return value[1:-1].replace('\\"', '"').replace("\\\\", "\\")
    return value


def split_responses(corpus: bytes) -> list[bytes]:
    """Mirrors ``fast::split_responses`` including literal framing."""
    responses: list[bytes] = []
    rest = corpus
    while rest:
        pos = 0
        end = None
        while True:
            newline = rest.find(b"\n", pos)
            if newline < 0:
                break
            line = rest[pos:newline]
            if line.endswith(b"\r"):
                line = line[:-1]
            match = re.search(rb"\{(\d+)\+?\}$", line)
            if match:
                data_start = newline + 1
                data_end = data_start + int(match.group(1))
                if data_end > len(rest):
                    break
                pos = data_end
            else:
                end = newline + 1
                break
        if end is None:
            break
        responses.append(rest[:end])
        rest = rest[end:]
    return responses


def _code(text: str | None) -> str | None:
    if not text:
        return None
    return re.split(r"[ \]]", text.strip(), maxsplit=1)[0]


def fingerprint(response: bytes) -> dict:
    """Returns the same normalised view as ``fuckthunder_tools::fingerprint``."""
    out: dict = {
        "kind": "",
        "keyword": None,
        "status": None,
        "code": None,
        "number": None,
        "seq": None,
        "uid": None,
        "mailbox": None,
        "count": None,
        "messages": None,
    }
    if response.startswith(b"+"):
        out["kind"] = "continuation"
        match = re.match(rb"^\+(?: \[([^\]]+)\])?(?: (.*))?\r\n$", response, re.S)
        if not match:
            raise ValueError(f"bad continuation: {response!r}")
        out["code"] = _code(_text(match.group(1)))
        out["keyword"] = _text(match.group(2)) or ""
        return out
    if not response.startswith(b"*"):
        out["kind"] = "tagged"
        match = re.match(
            rb"^(\S+) (OK|NO|BAD)(?: \[([^\]]+)\])?(?: (.*))?\r\n$", response, re.S
        )
        if not match:
            raise ValueError(f"bad tagged response: {response!r}")
        out["status"] = _text(match.group(2))
        out["code"] = _code(_text(match.group(3)))
        out["keyword"] = _text(match.group(4)) or ""
        return out

    out["kind"] = "untagged"
    text = response.decode("latin-1")
    # Numeric untagged responses.
    match = re.match(r"^\* (\d+) (EXISTS|RECENT|EXPUNGE)\r\n$", text)
    if match:
        out["keyword"] = match.group(2)
        out["number"] = int(match.group(1))
        return out
    # FETCH, including literal payloads.
    match = re.match(r"^\* (\d+) FETCH \((.*)\)\r\n$", text, re.S)
    if match:
        out["keyword"] = "FETCH"
        out["seq"] = int(match.group(1))
        uid = re.search(r"\bUID (\d+)", match.group(2))
        out["uid"] = int(uid.group(1)) if uid else None
        out["count"] = len(re.findall(r"\b(?:BODY\[[^\]]*\]|RFC822(?:\.HEADER|\.TEXT)?)\s+(?:\{\d+\+?\}\r\n)?", match.group(2)))
        return out
    if text.startswith("* FLAGS "):
        out["keyword"] = "FLAGS"
        return out
    match = re.match(r"^\* SEARCH(?: (.*))?\r\n$", text)
    if match:
        out["keyword"] = "SEARCH"
        ids = (match.group(1) or "").split()
        out["count"] = len(ids)
        return out
    match = re.match(r"^\* CAPABILITY .*\r\n$", text)
    if match:
        out["keyword"] = "CAPABILITY"
        return out
    match = re.match(
        r'^\* (LIST|LSUB) \(.*?\) ("(?:[^"\\]|\\.)*"|NIL) ("(?:[^"\\]|\\.)*")\r\n$',
        text,
    )
    if match:
        out["keyword"] = match.group(1)
        out["mailbox"] = decode_mutf7(unquote(match.group(3)))
        return out
    match = re.match(
        r'^\* STATUS ("(?:[^"\\]|\\.)*"|\S+) \((.*)\)\r\n$', text
    )
    if match:
        out["keyword"] = "STATUS"
        out["mailbox"] = decode_mutf7(unquote(match.group(1)))
        items = match.group(2).split()
        for index in range(0, len(items) - 1, 2):
            if items[index].upper() == "MESSAGES":
                out["messages"] = int(items[index + 1])
        return out
    match = re.match(r"^\* (OK|NO|BAD|BYE)(?: \[([^\]]+)\])?(?: (.*))?\r\n$", text, re.S)
    if match:
        out["keyword"] = match.group(1)
        out["code"] = _code(match.group(2))
        return out
    match = re.match(r"^\* PREAUTH(?: (.*))?\r\n$", text, re.S)
    if match:
        out["keyword"] = "PREAUTH"
        out["status"] = "PREAUTH"
        return out
    match = re.match(r"^\* NAMESPACE .*\r\n$", text, re.S)
    if match:
        out["keyword"] = "NAMESPACE"
        return out
    match = re.match(r"^\* ENABLED (.*)\r\n$", text)
    if match:
        out["keyword"] = "ENABLED"
        out["count"] = len(match.group(1).split())
        return out
    match = re.match(r"^\* ID \((.*)\)\r\n$", text, re.S)
    if match:
        out["keyword"] = "ID"
        out["count"] = len(re.findall(r'"(?:[^"\\]|\\.)*"\s+(?:NIL|"(?:[^"\\]|\\.)*")', match.group(1)))
        return out
    match = re.match(r"^\* QUOTA .*\r\n$", text, re.S)
    if match:
        out["keyword"] = "QUOTA"
        return out
    match = re.match(r"^\* (\w+).*\r\n$", text, re.S)
    if match:
        out["keyword"] = match.group(1)
        return out
    raise ValueError(f"unmodelled response: {response!r}")


def _text(value: bytes | None) -> str | None:
    return None if value is None else value.decode("latin-1")


def generate(seed: int, count: int) -> bytes:
    """Generates a deterministic corpus covering every modelled response kind."""
    rng = random.Random(seed)
    lines: list[bytes] = []
    tag = 0

    def mailbox() -> str:
        return rng.choice(MAILBOXES)

    templates = [
        lambda: f"* {rng.randint(1, 999)} EXISTS\r\n",
        lambda: f"* {rng.randint(0, 99)} RECENT\r\n",
        lambda: f"* {rng.randint(1, 999)} EXPUNGE\r\n",
        lambda: "* FLAGS (\\Seen \\Answered \\Flagged)\r\n",
        lambda: "* SEARCH " + " ".join(str(rng.randint(1, 999)) for _ in range(rng.randint(0, 5))) + "\r\n",
        lambda: "* CAPABILITY IMAP4rev1 IDLE AUTH=PLAIN AUTH=XOAUTH2\r\n",
        lambda: f'* LIST (\\HasNoChildren \\Sent) "/" "{mailbox()}"\r\n',
        lambda: f'* LSUB () "/" "{mailbox()}"\r\n',
        lambda: f'* STATUS "{mailbox()}" (MESSAGES {rng.randint(0, 500)} UIDNEXT {rng.randint(1, 500)} UIDVALIDITY 7 UNSEEN {rng.randint(0, 50)})\r\n',
        lambda: f'* {rng.randint(1, 999)} FETCH (UID {rng.randint(1, 999)} FLAGS (\\Seen) RFC822.SIZE {rng.randint(1, 100000)} INTERNALDATE "17-Jul-1996 02:44:25 -0700")\r\n',
        lambda: "* NAMESPACE ((\"\" \"/\")) NIL ((\"Shared/\" \"/\"))\r\n",
        lambda: "* ENABLED CONDSTORE UTF8=ACCEPT\r\n",
        lambda: '* ID ("name" "fuckthunder" "version" "0.1")\r\n',
        lambda: f'* QUOTA "" (STORAGE {rng.randint(0, 1000)} 2048)\r\n',
        lambda: '* ESEARCH (TAG "a1") UID ALL 1:5\r\n',
        lambda: f'* OK [{rng.choice(CODES)}] greeting text\r\n',
        lambda: "* NO [TRYCREATE] no such mailbox\r\n",
        lambda: "* BAD unexpected command\r\n",
        lambda: "* BYE [UNAVAILABLE] shutting down\r\n",
        lambda: "* PREAUTH ready\r\n",
        lambda: f"tag{tag} OK [READ-WRITE] command completed\r\n",
        lambda: f"tag{tag} NO [TRYCREATE] rejected\r\n",
        lambda: f"tag{tag} BAD malformed\r\n",
        lambda: "+ Ready for literal data\r\n",
    ]

    def literal_fetch() -> bytes:
        payload = ("hello\r\n" + str(rng.randint(0, 999))).encode()
        return (
            f"* {rng.randint(1, 999)} FETCH (UID {rng.randint(1, 999)} BODY[] "
            f"{{{len(payload)}}}\r\n".encode()
            + payload
            + b")\r\n"
        )

    # Guarantee at least one of every template, then fill deterministically.
    for make in templates:
        lines.append(make().encode())
    while len(lines) < count:
        if rng.random() < 0.25:
            lines.append(literal_fetch())
        else:
            lines.append(templates[rng.randrange(len(templates))]().encode())
    rng.shuffle(lines)
    return b"".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    gen = sub.add_parser("generate")
    gen.add_argument("--seed", type=int, required=True)
    gen.add_argument("--count", type=int, default=120)
    gen.add_argument("--out", type=Path, required=True)
    fps = sub.add_parser("fingerprints")
    fps.add_argument("--input", type=Path, required=True)
    fps.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    if args.command == "generate":
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_bytes(generate(args.seed, args.count))
        return 0

    corpus = args.input.read_bytes()
    prints = [fingerprint(response) for response in split_responses(corpus)]
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(prints, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
