# ADR-0001: Thumbnail format is JPEG

Date: 2026-08-30 · Status: accepted · Milestone: 2

## Context

PRD section 14 left the thumbnail format open: "WebP is smaller, JPEG has
zero dependency risk. Decide during milestone 2." Thumbnails are 256px long
edge, cached on disk keyed by content hash (section 5.1).

## Decision

JPEG, quality 80, encoded by the `image` crate (already a listed dependency).

## Rationale

- Zero new dependencies: the `image` crate's lossy WebP support would need
  libwebp bindings, which are not on the PRD's crate list. Its pure-Rust WebP
  encoder is lossless-only, which defeats the size argument.
- At 256px a JPEG q80 thumbnail is ~10-20 KB; for a 300k library that is
  ~3-6 GB → ~4 GB. The WebP saving (~25-30%) is real but not decisive.
- Everything decodes JPEG: Swift/AppKit previews, Quick Look, Vision.

## Consequences

Thumbnails live at `<db dir>/thumbs/<first 2 hex>/<64 hex>.jpg`. If disk
size ever matters, a future migration can re-encode the cache; the content
hash key and schema (`images.thumb_path`) don't change.
