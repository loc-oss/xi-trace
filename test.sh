#!/usr/bin/env bash

LOG_FILE="cargo_pipeline.log"
: > "$LOG_FILE"
failed=0

steps=(
    "test|cargo test"
    "test --all-features|cargo test --all-features"
    "test --no-default-features|cargo test --no-default-features"
    "test --no-default-features --features json_payload|cargo test --no-default-features --features json_payload"
    "clippy|cargo clippy --all-features --all-targets -- -D warnings"
)

for step in "${steps[@]}"; do
    name="${step%%|*}"
    cmd="${step#*|}"
    echo ">>> $name" | tee -a "$LOG_FILE"
    eval "$cmd" >> "$LOG_FILE" 2>&1
    code=$?
    echo "    exit=$code" | tee -a "$LOG_FILE"
    [ $code -ne 0 ] && failed=1
done

echo "log: $LOG_FILE"
exit $failed