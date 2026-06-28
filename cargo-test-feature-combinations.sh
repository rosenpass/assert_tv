#!/usr/bin/env bash
set -euo pipefail

combos=(
    ""
    "tls"
    "zstd-offload"
    "tls,zstd-offload"
)

for features in "${combos[@]}"; do
    echo ">>> Testing with features: [${features:-<none>}]"
    echo "=============================================="
    cargo test -p assert_tv --no-default-features --features "$features"
    cargo test -p example   --no-default-features --features "$features"
done

echo "All feature combinations passed."
