# ADR-0004: CLIP embeddings computed from cached thumbnails

Date: 2026-08-30 · Status: accepted · Milestone: 4

## Context

CLIP ViT-B/32 consumes a 224x224 crop. The analysis pass already decodes
every original once and caches a 256px-long-edge thumbnail keyed by content
hash. Re-decoding multi-megapixel originals in the embedding pass would
double the most expensive I/O in the pipeline.

## Decision

The embedding stage reads the cached thumbnail, not the original. Files
without a thumbnail (HEIC/RAW per ADR-0002, corrupt files) are not embedded.

## Consequences

- Embedding throughput is bounded by the model, not JPEG decode — helps the
  40 images/sec target (PRD §10).
- Slight fidelity loss: a wide thumbnail's short side can be under 224px and
  gets upscaled. Immaterial for CLIP-scale semantic search.
- Deleting the thumbnail cache does not invalidate stored embeddings; the
  embeddings table keys off file id and is dropped with the images row on
  content change, same as all derived facts.
