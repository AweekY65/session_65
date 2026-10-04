#!/usr/bin/env bash
# 等价于 cargo run --bin verify_regex 的测试脚本入口。
set -euo pipefail
cd "$(dirname "$0")"
cargo run --release --bin verify_regex
