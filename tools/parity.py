#!/usr/bin/env python3
"""A/B differential parity runner.

For each deterministic seed it:

1. generates an IMAP corpus with the legacy-derived Python model,
2. fingerprints it with the Python model,
3. fingerprints the same bytes with the Rust ``fh-parity`` binary,
4. compares the two JSON documents exactly.

Exits non-zero on the first divergence and prints the offending response index.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODEL = ROOT / "tools" / "legacy_model.py"


def run(command: list[str], **kwargs) -> None:
    subprocess.run(command, check=True, **kwargs)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seeds", type=int, default=20)
    parser.add_argument("--count", type=int, default=120)
    parser.add_argument("--workdir", type=Path, default=ROOT / "fixtures" / "parity")
    parser.add_argument(
        "--rust-bin",
        type=Path,
        default=ROOT / "target" / "release" / "fh-parity",
    )
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()

    if not args.skip_build:
        run(
            [
                "cargo",
                "build",
                "--release",
                "-p",
                "fuckthunder-tools",
                "--bin",
                "fh-parity",
            ],
            cwd=ROOT,
        )

    if not args.rust_bin.exists():
        print(f"missing Rust parity binary: {args.rust_bin}", file=sys.stderr)
        return 2

    args.workdir.mkdir(parents=True, exist_ok=True)
    summary = {
        "seeds_tested": args.seeds,
        "responses_per_seed": args.count,
        "seeds": [],
        "max_abs_error": "0",
    }

    for seed in range(args.seeds):
        corpus = args.workdir / f"seed_{seed:03d}.imap"
        legacy = args.workdir / f"seed_{seed:03d}.legacy.json"
        rust = args.workdir / f"seed_{seed:03d}.rust.json"

        run(
            [
                sys.executable,
                str(MODEL),
                "generate",
                "--seed",
                str(seed),
                "--count",
                str(args.count),
                "--out",
                str(corpus),
            ]
        )
        run(
            [
                sys.executable,
                str(MODEL),
                "fingerprints",
                "--input",
                str(corpus),
                "--out",
                str(legacy),
            ]
        )
        run(
            [
                str(args.rust_bin),
                "--input",
                str(corpus),
                "--out",
                str(rust),
            ]
        )

        left = json.loads(legacy.read_text())
        right = json.loads(rust.read_text())
        if left != right:
            for index, (a, b) in enumerate(zip(left, right)):
                if a != b:
                    print(
                        f"seed {seed}: response {index} diverged\n"
                        f"  legacy: {json.dumps(a, sort_keys=True)}\n"
                        f"  rust:   {json.dumps(b, sort_keys=True)}",
                        file=sys.stderr,
                    )
                    break
            else:
                print(
                    f"seed {seed}: length mismatch "
                    f"(legacy {len(left)}, rust {len(right)})",
                    file=sys.stderr,
                )
            summary["seeds"].append({"seed": seed, "status": "fail"})
            (args.workdir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
            return 1

        summary["seeds"].append(
            {"seed": seed, "status": "pass", "responses": len(left)}
        )
        print(f"seed {seed:03d}: {len(left)} responses matched")

    (args.workdir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    total = sum(item.get("responses", 0) for item in summary["seeds"])
    print(f"parity: {args.seeds}/{args.seeds} seeds, {total} responses, max_abs_error=0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
