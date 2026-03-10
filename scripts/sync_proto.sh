#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROTO_DIR="$ROOT_DIR/proto/upstream"
CRATE_PROTO_DIR="$ROOT_DIR/crates/aelf-proto/proto/upstream"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}

trap cleanup EXIT

git clone --depth 1 https://github.com/AElfProject/aelf-sdk.cs "$TMP_DIR/aelf-sdk.cs"

mkdir -p "$PROTO_DIR"
rsync -av --delete \
  "$TMP_DIR/aelf-sdk.cs/src/AElf.Client.Protobuf/Protobuf/" \
  "$PROTO_DIR/"

mkdir -p "$CRATE_PROTO_DIR"
rsync -av --delete "$PROTO_DIR/" "$CRATE_PROTO_DIR/"

AUTHORITY_INFO_FILE="$PROTO_DIR/authority_info.proto"
if ! grep -q '^package aelf;' "$AUTHORITY_INFO_FILE"; then
  python3 - "$AUTHORITY_INFO_FILE" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
content = path.read_text()
needle = 'syntax = "proto3";\n\n'
replacement = 'syntax = "proto3";\n\npackage aelf;\n\n'
if needle in content and "package aelf;" not in content:
    path.write_text(content.replace(needle, replacement, 1))
PY
fi

python3 - "$PROTO_DIR" <<'PY'
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
pattern = re.compile(r'(?<![\w.])AuthorityInfo(?!\w)')

for path in root.rglob("*.proto"):
    if path.name == "authority_info.proto":
        continue
    content = path.read_text()
    updated = pattern.sub("aelf.AuthorityInfo", content)
    if updated != content:
        path.write_text(updated)
PY

echo "Synced proto files into $PROTO_DIR and $CRATE_PROTO_DIR"
