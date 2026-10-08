# bcaip-sdk

Python bindings for the bcaip Development Kit (BDK).

This package is generated from the Rust `bcaip-sdk` crate using UniFFI.

## Build a local wheel

From the repository root:

```bash
just --justfile crates/bcaip-sdk/justfile python-wheel
```

The wheel is written to `crates/bcaip-sdk/python/dist/`.
