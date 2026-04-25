#!/usr/bin/env bash
set -euo pipefail

python -m grpc_tools.protoc \
  -I proto \
  --python_out=adapter-python/aria_underlay_adapter/proto \
  --grpc_python_out=adapter-python/aria_underlay_adapter/proto \
  proto/aria_underlay_adapter.proto

