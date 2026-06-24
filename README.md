# assert_tv: Deterministic tests with external test vectors

assert_tv helps you capture, persist, and validate test vectors so that non-deterministic code (randomness, time, OS input) can be tested deterministically. It generates a file alongside your tests, recording inputs (“consts”) and outputs, and then replays and verifies them on subsequent runs.

This README documents the current API and usage based on the latest changes applied in downstream projects.

## Highlights

- **Ergonomic test fields**: Define a small struct of `TestValue<…>` and derive `TestVectorSet`.
- **Opt-in at call sites**: Parameterize functions with `TV: TestVector` and use `TestVectorActive` (tests) or `TestVectorNOP` (production).
- **One-line test harness**: Use `#[test_vec_case(...)]` to auto-initialize, run, and finalize a test-vector-backed test.
- **Portable formats**: JSON (default), YAML, TOML.
- **Large values support**: Offload large entries to compressed sidecar files.

## Installation

Add the crate from crates.io (no special features required):

```toml
[dependencies]
assert_tv = "0.6"
```

If you only use it from tests, you can put it under `[dev-dependencies]`.

## Quick Start

1) Define your test fields once and derive `TestVectorSet`.

```rust
use assert_tv::{TestValue, TestVectorSet};

#[derive(TestVectorSet)]
struct Fields {
    #[test_vec(name = "rand", description = "random component")]
    rand: TestValue<u64>,

    #[test_vec(name = "sum")]
    sum: TestValue<u64>,
}
```

2) Parameterize code under test with `TV: TestVector` and expose/check values.

```rust
use assert_tv::{TestVector, TestVectorActive};

fn add_with_random<TV: TestVector>(a: u64, b: u64) -> u64 {
    let tv = TV::initialize_values::<Fields>();

    let r = rand::random::<u64>();          // nondeterministic
    let r = TV::expose_value(&tv.rand, r);   // recorded and replayed

    let out = a + b + r;
    TV::check_value(&tv.sum, &out);          // verified in check mode
    out
}
```

3) Wrap tests with `#[test_vec_case]` to manage setup/teardown and file I/O.

```rust
use assert_tv::test_vec_case;

#[test_vec_case]                        // default: .test_vectors/test_add_with_random.json
fn test_add_with_random() {
    let out = add_with_random::<TestVectorActive>(2, 3);
    assert!(out >= 5);
}
```

First run in init mode to create vectors, then use check mode to validate:

```bash
TEST_MODE=init  cargo test -- --exact test_add_with_random
TEST_MODE=check cargo test -- --exact test_add_with_random
```

By default the test vector file is placed at `.test_vectors/<fn_name>.json`. You can customize file and format:

```rust
#[test_vec_case(file = "tests/vecs/add.yaml", format = "yaml")]
fn test_add_with_random_yaml() { /* ... */ }
```

## How It Works

- **Test fields**: A `#[derive(TestVectorSet)]` struct contains `TestValue<T>` fields. Each field carries metadata and (by default) serde-based serializers.
- **Exposing values**: `TV::expose_value(&field, value)` records a “Const” entry and returns the loaded value in check/init, enabling de-randomization; with `TestVectorNOP` it simply returns the original value.
- **Checking values**: `TV::check_value(&field, &value)` records an “Output” entry and, in check mode, compares it against the stored vector.
- **Test harness**: `#[test_vec_case(...)]` wraps your test function, calling `initialize_tv_case_from_file(...)` on entry and `finalize_tv_case()` on exit. The mode comes from the attribute (`mode = "init" | "check"`) or, if omitted, from `TEST_MODE` (default is check).

## Field Attributes

Annotate fields with `#[test_vec(...)]` to control metadata and serialization:

- **name**: human-readable key (string)
- **description**: longer description (string)
- **serialize_with**: path to `fn(&T) -> anyhow::Result<serde_json::Value>`
- **deserialize_with**: path to `fn(&serde_json::Value) -> anyhow::Result<T>`
- **offload**: `true` to keep large data out of the main file; values are written to `"<file>_offloaded_value_<index>.zstd"` and the main file stores `null` for that entry

Example:

```rust
#[derive(TestVectorSet)]
struct Fields {
    #[test_vec(name = "payload", description = "large blob", offload = true)]
    payload: TestValue<Vec<u8>>,
}
```

## Manual Setup (advanced)

If you are outside of a test or need custom control, initialize and finalize explicitly:

```rust
use assert_tv::{initialize_tv_case_from_file, finalize_tv_case, TestMode, TestVectorFileFormat};

let _guard = initialize_tv_case_from_file(
    "tests/vecs/case.toml",
    TestVectorFileFormat::Toml,
    TestMode::Init,
).expect("init tv");

// ... run code that uses TestVectorActive/TestVectorNOP generics ...

finalize_tv_case().expect("finalize tv");
drop(_guard);
```

## Production Transparency

In production, choose `TestVectorNOP` so calls compile down to pass-through/no-ops:

```rust
use assert_tv::{TestVector, TestVectorNOP};

fn compute<TV: TestVector>(x: i32) -> i32 {
    let tv = TV::initialize_values::<Fields>();
    let a = TV::expose_value(&tv.rand, rand::random()); // pass-through with NOP
    let y = x + a;
    TV::check_value(&tv.sum, &y);                      // no-op with NOP
    y
}

// production call
let _ = compute::<TestVectorNOP>(42);
```

No special Cargo features are required to “enable” assert_tv for tests. Switching between active and no-op behavior is driven by the `TV` generic (`TestVectorActive` in tests vs. `TestVectorNOP` elsewhere). A `tls` feature is available (enabled by default) to back the environment with thread-local storage; without it, a global mutex-based storage is used.

## Modes

- **Init**: records observed entries and writes the vector file (only updates if missing or changed).
- **Check**: loads the vector file and validates observed entries; constants are injected from file.

Set via `#[test_vec_case(mode = "init" | "check")]` or the `TEST_MODE` environment variable (defaults to check).

## Formats

- **JSON** (default), **YAML**, **TOML**.

Choose with `#[test_vec_case(format = "json" | "yaml" | "toml")]` or when calling `initialize_tv_case_from_file` directly.

## Cargo Features

The `assert_tv` crate has two features that are enabled by default:

- **`tls`**: Store the active test-vector session in thread-local storage, so each test thread has its own isolated session and tests run in parallel. When disabled, a single global session is used and a process-wide lock serializes test cases.
- **`zstd-offload`**: Compress offloaded values with zstd before writing their sidecar files. When disabled, offloaded values are written uncompressed.

## Notes

- The default test vector path is `.test_vectors/<function_name>.<format>` when using `#[test_vec_case]`.
- Values marked `offload = true` are stored next to the main file and compressed with zstd.
- Custom serializers/deserializers let you normalize or prettify complex types before persistence.

```rust
// production code:
let a = &mut vec![0;8];
a[..4].copy_from_slice(
    &[rand::random::<u8>(), rand::random::<u8>(), rand::random::<u8>(), rand::random::<u8>()]
);
// tv integration:
```
