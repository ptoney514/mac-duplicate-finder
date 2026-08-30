# ADR-0005: arm64-only builds from milestone 4

Date: 2026-08-30 · Status: accepted · Milestone: 4

## Context

PRD §12 calls for universal (aarch64 + x86_64) builds. Milestone 4 adds
ONNX Runtime via `ort`, whose `ort-sys` no longer provides prebuilt macOS
x86_64 binaries — upstream onnxruntime dropped Intel-mac support. Building
ONNX Runtime from source for x86_64 is a heavy toolchain (cmake, python,
long builds) for an architecture this project doesn't run on: the PRD's
target machine is an Apple Silicon Mac mini, and the performance targets
(§10) assume the CoreML EP.

## Decision

The XCFramework and release builds are aarch64-apple-darwin only.

## Consequences

- `scripts/build-xcframework.sh` keeps its multi-target structure; re-adding
  `x86_64-apple-darwin` to `TARGETS` is the only change needed if Intel
  prebuilts return or a source build is ever justified.
- No user-facing impact on the target machine.
