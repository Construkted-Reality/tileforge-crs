# tileforge-crs documentation

> **Status:** Current.
> **Summary:** Map of the tileforge-crs documents, the open items, the consumer pins, and the locations of decisions that source comments cite.

Each document starts with a **Status** line and a **Summary** line. Status is Current
for shipped behavior or an active rule, Proposed for an unimplemented design, and
Historical for dated evidence.

Markdown files longer than 100 lines, excluding `CHANGELOG.md`, have a generated
line-numbered contents block near the top. The title, status, summary, and complete
contents block fit within the first 100 lines.

Removed documents are listed in [REMOVED.md](REMOVED.md) with the commit that still
contains them. After you edit a long document, regenerate the contents blocks with
`python3 <tileforge umbrella>/scripts/docs/doc-index.py index .`, and check them and
the relative links with `python3 <tileforge umbrella>/scripts/docs/doc-index.py check .`.

Family conventions (build hosts, pinning rule, review process) live in the umbrella
repository `tileforge-workspace`, under `docs/`. This repository documents only the
behavior of this crate. The API reference is the rustdoc in `src/lib.rs`.

| Document | Status | Read when |
|---|---|---|
| [reviews/2026-09-07-explicit-transforms.md](reviews/2026-09-07-explicit-transforms.md) | Historical | You change WKT parsing or datum-operation handling. |
| [worklog/2026-07-03.md](worklog/2026-07-03.md) | Historical | You want the reason for an input guard before you loosen it. |
| [REMOVED.md](REMOVED.md) | Current | You need a document that was deleted as stale. |

## Open items

- Release 0.3.0 has no Git tag. The only tag is `v0.2.0`. The 0.3.0 release commit is `5a8f749`.
- MT1 remainder: no frozen PROJ fixture for a non-WGS84 geographic source (EPSG:4269) and no EPSG:4979 grid. Both need a machine with PROJ installed.
- `EPSG:+123` parses as 123, because `u16::parse` accepts a leading `+` (`src/sidecar.rs`). Harmless; tighten it if the parser changes.

## Cross-repository references

Source comments cite decisions that live in the consumer repositories:

- `ADR-002`, `ADR-004`, `ADR-013`: `tileforge-pc/docs/adr/`.
- `ADR-041` (and its C1, D2, D3, R1 items): `tileforge-mesh/docs/design/adr/`.
- "spike 0g": `tileforge-pc/spikes/0g-crs-crate/`.

## Consumer pins

Consumers pin this crate by Git revision in `tileforge-mesh/Cargo.toml` and
`tileforge-pc/crates/tileforge-pc-crs/Cargo.toml`. Read those files for the current pins.
A version number in `CHANGELOG.md` does not tell you what a consumer builds.

Pins on each consumer's `origin/main`, checked 2026-10-05:

| Consumer | Pin | Contents |
|---|---|---|
| `tileforge-mesh` | `1d279f6` | Sentinel rejection and `BOUNDCRS`/`TOWGS84` rejection. Not the unit accessors or `source_crs`. |
| `tileforge-pc` | `9df2ab2` | All 0.3.0 code changes. |
