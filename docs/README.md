# tileforge-crs documentation

Each document starts with a **Status** line (Current or Historical) and a **Summary** line.
Files of 200 lines or more also have a line-numbered contents block at the top.
Removed documents are listed in [REMOVED.md](REMOVED.md) with the commit that still contains them. Regenerate the contents blocks after you edit a long document: `python3 <tileforge umbrella>/scripts/docs/doc-index.py index .` (check with `... check .`).

| Document | Status | Read when |
|---|---|---|
| [reviews/2026-09-07-explicit-transforms.md](reviews/2026-09-07-explicit-transforms.md) | Historical | You change WKT parsing or datum-operation handling. |
| [worklog/2026-07-03.md](worklog/2026-07-03.md) | Historical | You want the reason for an input guard before you loosen it. |
| [REMOVED.md](REMOVED.md) | Current | You need a document that was deleted as stale. |

## Open items

- MT1 remainder: no frozen PROJ fixture for a non-WGS84 geographic source (EPSG:4269) and no EPSG:4979 grid. Both need a machine with PROJ installed.
- `EPSG:+123` parses as 123, because `u16::parse` accepts a leading `+` (`src/sidecar.rs`). Harmless; tighten it if the parser changes.

## Cross-repository references

Source comments cite decisions that live in the consumer repositories:

- `ADR-002`, `ADR-004`, `ADR-013`: `tileforge-pc/docs/adr/`.
- `ADR-041` (and its C1, D2, D3, R1 items): `tileforge-mesh/docs/design/adr/`.
- "spike 0g": `tileforge-pc/spikes/0g-crs-crate/`.

Consumers pin this crate in `tileforge-mesh/Cargo.toml` and `tileforge-pc/crates/tileforge-pc-crs/Cargo.toml`.
