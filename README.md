# xi-trace

[![CI](https://github.com/loc-oss/xi-trace/actions/workflows/ci.yml/badge.svg)](https://github.com/loc-oss/xi-trace/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Library-based distributed tracing API designed to meet the needs of `xi-core`, frontends, and plugins.

`xi-trace` provides a lightweight, low-overhead way to instrument your Rust code. It generates trace files compatible with the [Chrome Trace Event Format](https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU/preview), allowing you to visualize performance bottlenecks in Chrome's `about:tracing` viewer or any compatible tool (e.g., Perfetto).

## Features

- **Low Overhead:** Designed to be fast when tracing is disabled.
- **Chrome Trace Compatible:** Outputs standard JSON trace files.
- **Flexible Instrumentation:** Trace instantaneous events, blocks of code, or closures.
- **Payload Support:** Attach custom string or JSON payloads to your traces.
- **Thread-Safe:** Built on top of `Mutex` and `AtomicBool` for safe concurrent use.
- **API Stability:** Locked in with extensive API-compatibility tests (`tests/api_compat.rs`).
- **Rust 2024 Edition:** Fully migrated and modernized.

## Usage

Add `xi-trace` to your `Cargo.toml`:

```toml
[dependencies]
xi-trace = "0.3.0"
```

### Basic Example

```rust
fn main() {
    // 1. Enable tracing (by default, it stores up to 1 MiB of samples)
    xi_trace::enable_tracing();

    // 2. Trace an instantaneous event
    xi_trace::trace("app_started", &["lifecycle", "init"]);

    // 3. Trace a block of code (duration is recorded when `_guard` is dropped)
    {
        let _guard = xi_trace::trace_block("heavy_computation", &["cpu"]);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // 4. Trace a closure (returns the closure's return value)
    let result = xi_trace::trace_closure("calculate", &["math"], || {
        42 + 42
    });

    // 5. Save the trace to a JSON file (sorted chronologically)
    xi_trace::save("trace.json", true).expect("Failed to save trace");
    
    println!("Result: {}", result);
    println!("Open trace.json in Chrome via about:tracing");
}
```

### Custom Configuration

You can configure the maximum number of samples or the total memory limit:

```rust
use xi_trace::Config;

// Limit by sample count
xi_trace::enable_tracing_with_config(Config::with_limit_count(10_000));

// Or limit by bytes (rounds down to the nearest multiple of Sample size)
xi_trace::enable_tracing_with_config(Config::with_limit_bytes(5 * 1024 * 1024)); // 5 MiB
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `chrome_trace_event` | ✅ | Enables the `chrome_trace_dump` module and `save()` functionality. |
| `json_payload` | ❌ | Changes trace payloads from `StrCow` to `serde_json::Value`. |
| `ipc` | ❌ | Adds `bincode` support for IPC serialization. |
| `getpid` | ❌ | (Internal) Exposes process ID retrieval. |

To use JSON payloads:
```toml
[dependencies]
xi-trace = { version = "0.3.0", features = ["json_payload"] }
```

## Development & Testing

The project is strictly protected against regressions via a comprehensive test suite. To run the same checks as our CI pipeline:

```bash
cargo test
cargo test --all-features
cargo test --no-default-features
cargo test --no-default-features --features json_payload
cargo clippy --all-features --all-targets -- -D warnings
```

Or simply run the provided script:
```bash
chmod +x test.sh
./test.sh
```

## License

Licensed under the Apache License, Version 2.0. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE) for details.