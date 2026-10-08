# bcaip-local-inference Cleanup

This crate is not yet in a maintainable BCAIP shape. Keep refactors incremental,
compiled, and domain-oriented. Do not hide design debt behind feature gates or
parallel fallback implementations.

## Rules

- No `use super`, `super::`, or glob imports.
- Prefer one important domain type per file, named after the type.
- A backend unavailable on a platform is not a second backend implementation.
- Keep feature/platform differences at capability boundaries, not duplicated
  business logic.
- Do not move unrelated API surfaces while cleaning internals.
- Run `cargo fmt -p bcaip-local-inference`.
- At minimum check:
  - `cargo check -p bcaip-local-inference`
  - `cargo check -p bcaip-local-inference --features mlx`
  - `cargo check -p bcaip-local-inference --features hf-hub`

## Current State

- MLX no longer has separate available/unavailable backend modules.
- `MlxBackend` still contains too much generation orchestration and should be
  thinned further.
- `hf_models.rs` is the worst file: search, catalog DTOs, GGUF parsing, MLX
  metadata, downloads, cache inspection, and deletion are mixed in one file.
- `lib.rs`, `management.rs`, and `tool_emulation.rs` are also too large.

## Cleanup Order

1. Split `hf_models.rs` domain types into focused files.
2. Split GGUF quantization/parsing helpers out of `hf_models.rs`.
3. Split HF download progress/cache deletion out of `hf_models.rs`.
4. Thin `mlx_backend.rs` by moving request preparation and usage/logging helpers.
5. Thin `lib.rs` runtime/provider responsibilities into focused modules.
6. Thin `management.rs` DTO mapping and command handlers.

## Last Known Validation

The crate passed the three checks above after the MLX availability cleanup.
