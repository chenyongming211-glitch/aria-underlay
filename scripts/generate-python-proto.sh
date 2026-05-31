#!/usr/bin/env bash
set -euo pipefail

PYTHON_BIN="${PYTHON:-python3}"

"$PYTHON_BIN" -m grpc_tools.protoc \
  -I proto \
  --python_out=adapter-python/aria_underlay_adapter/proto \
  --grpc_python_out=adapter-python/aria_underlay_adapter/proto \
  proto/aria_underlay_adapter.proto
