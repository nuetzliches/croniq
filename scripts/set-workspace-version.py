#!/usr/bin/env python3
"""Rewrite `[workspace.package].version` in Cargo.toml, in place.

Usage: scripts/set-workspace-version.py <version>

Used by ci.yml's Docker job to stamp main-branch images with
`<last release>+main.<sha>` (review 2026-10-24, C-2). release.yml does the
same rewrite inline with the same pattern to stamp the tag. The edit is local
to the runner and never committed.

Only the first `version = "..."` after the `[workspace.package]` header is
touched, so per-crate manifests and dependency versions stay as they are. The
pattern does not cross a `[`, so the comment above the key must not contain
one either -- the script exits non-zero rather than rewriting nothing.
"""

import re
import sys

if len(sys.argv) != 2 or not sys.argv[1]:
    sys.exit("usage: set-workspace-version.py <version>")
version = sys.argv[1]
# Cargo accepts semver with pre-release and build metadata; anything else
# would only fail later, inside the build, with a less obvious message.
if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?", version):
    sys.exit(f"not a semver version: {version!r}")

with open("Cargo.toml", encoding="utf-8") as f:
    src = f.read()
new = re.sub(
    r'(\[workspace\.package\][^\[]*?\nversion = )"[^"]+"',
    lambda m: m.group(1) + f'"{version}"',
    src,
    count=1,
    flags=re.DOTALL,
)
if new == src:
    sys.exit("failed to rewrite [workspace.package].version")
with open("Cargo.toml", "w", encoding="utf-8", newline="") as f:
    f.write(new)
print(f"[workspace.package].version = {version}")
