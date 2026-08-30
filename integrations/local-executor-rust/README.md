# Local executor Rust core

This crate is the allowlisted local media execution core for a future Tauri adapter. It is
intentionally independent of the current web and Go applications.

The public API accepts only three typed actions: deterministic test-clip generation,
MP4 transcoding, and media verification. The host registers trusted absolute root directories;
requests use a root ID and a validated relative path. FFmpeg and ffprobe are executed directly
with argument arrays, never through a shell.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --manifest-path integrations/local-executor-rust/Cargo.toml
cargo test --manifest-path integrations/local-executor-rust/Cargo.toml \
  --test ffmpeg_integration -- --ignored --nocapture
```

For a zero-cost manual sample, use a fresh temporary directory:

```sh
sample_dir="$(mktemp -d)"
cargo run --manifest-path integrations/local-executor-rust/Cargo.toml \
  --bin local-executor-demo -- sample "$sample_dir"
```
