# ADR-0006: Frontier tier targets an OpenAI-compatible endpoint

Date: 2026-08-30 · Status: accepted · Milestone: 7

## Context

PRD §5.3 specifies the Claude API for cluster tiebreaks. The user judged
Claude API pricing too expensive for this personal tool and asked for
OpenAI with the model "luna-5.6" — or a placeholder.

## Decision

The frontier client speaks the OpenAI Chat Completions wire format with a
fully configurable endpoint: base URL, model id, and API key are settings
(key in the Keychain). Defaults: `https://api.openai.com/v1`, model
`luna-5.6`. With no key entered the feature is inert — the placeholder and
the implementation are the same code.

Cost controls stay per §5.3: verdicts are requested only via the explicit
"Ask AI" button, only the top 4 thumbnails (256px JPEGs) plus quality
signals are sent, and verdicts are cached by cluster content hash so a
cluster is never sent twice.

## Consequences

- Any OpenAI-compatible server works (OpenAI, a proxy, a local server), so
  a wrong or renamed model id is a one-field fix in Settings.
- Verdict cache lives in SwiftData keyed by a hash of the member content
  hashes, not the PRD §6 `frontier_verdicts` table: stored cluster ids are
  regenerated on every recluster, so an id-keyed cache would never hit, and
  the API client lives on the Swift side anyway. The unused table stays for
  a future engine-side implementation.
