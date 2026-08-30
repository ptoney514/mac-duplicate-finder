# ADR-0002: HEIC/RAW pixel decode deferred to the Swift shell

Date: 2026-08-30 · Status: accepted · Milestone: 2 (revisit in 3/5)

## Context

The `image` crate cannot decode HEIC/HEIF or camera RAW (CR2/CR3/NEF/ARW/…).
Decoding them in Rust would need libheif/libraw bindings, which are not on
the PRD's dependency list (section 5.1), and section 15 says to ask before
adding dependencies.

## Decision

In milestone 2, files the `image` crate cannot decode are still walked,
content-hashed, and EXIF-parsed (kamadak-exif reads HEIF containers), and get
an `images` row with status `analyzed` — but with NULL dimensions, perceptual
hashes, and thumbnail. They participate fully in exact-duplicate detection
and never in near clustering.

Plan: when the Swift shell lands (milestone 3+), generate pixels for these
formats on the Apple side (ImageIO/CGImageSource decodes HEIC and most RAW
for free) and hand them to the engine for thumbnail + perceptual hashing, or
revisit adding libheif after asking.

## Consequences

- iPhone HEIC libraries get exact-dupe support now, near-dupe support once
  the Apple-side decode path exists.
- The `images` row schema already accommodates this (all pixel-derived
  columns nullable), so no migration will be needed.
- Decode failures are surfaced in `ScanSummary.errors` so the gap is visible
  in the CLI rather than silent.
