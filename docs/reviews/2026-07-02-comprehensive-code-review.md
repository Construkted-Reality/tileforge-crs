# Comprehensive code review — tileforge-crs

- **Date:** 2026-07-02
- **Commit:** d11c0fb42b7d78924860b779f4597c9d7e2eb712
- **Branch:** master
- **Reviewer:** Claude (agent session), read-only review
- **Coverage:** 100% of the crate read — `Cargo.toml`, all 8 files under `src/`
  (1,582 lines), both integration tests under `tests/` (210 lines), and the
  fixture headers. `cargo test`: 55/55 pass (46 unit + 9 integration).
  `cargo clippy --all-targets`: clean, zero warnings.
- **Empirical verification:** a probe crate (scratchpad, outside the repo) was
  built against this commit and exercised: WGS84 closed-form ground truth,
  poles, antimeridian, longitude wrapping, out-of-range latitude, NaN/inf
  propagation, geocenter/interior ECEF origins, a US-survey-feet projected CRS
  (EPSG:2926), an ENU-basis orthonormality sweep over lat −89.9..89.9 ×
  7 longitudes, 50 km ENU round-trips, and short-form parse corners. Findings
  below are marked **confirmed** (probe output) or **code-read**.

## Headline

The numerics are solid. Confirmed by probe:

- Geographic (EPSG:4326) → ECEF matches the closed-form WGS84 equations to
  **0.0 m delta** at mid-latitude, ±89.9999°, antimeridian, and h = −430 m /
  +8,000 m. Equator/pole/axis points match `a = 6378137` and
  `b = 6356752.314245` exactly (residuals < 1e-9 m).
- lon = 180 vs −180, and lon = 360/730/−540 wrap correctly (deltas ≤ 1e-9 m).
- ENU basis worst orthonormality deviation over the global sweep: **2.2e-16**
  (machine epsilon). 50 km ENU→ECEF→ENU round-trip error: **3.2e-10 m**.
- US-survey-feet projected CRS (EPSG:2926) converts with correct unit
  handling (Seattle test point lands at lon −122.35, lat 47.62).
- Out-of-range latitude (|lat| > 90°) errors cleanly (`LatitudeOutOfRange`).
- The parity oracles pin two UTM zones (17N northern, 19S southern
  hemisphere) against frozen PROJ 9.8.1 grids at <1 µm; both pass.

No critical bugs and no incorrect-math bugs were found. The real findings are
error-handling edges (silent NaN, degenerate origins), API-contract
documentation, and test-coverage gaps.

## Critical bugs

None found.

## Bugs

### F1 — `to_ecef` silently returns NaN for geographic sources when longitude or height is non-finite — **confirmed**

- **Refs:** `src/reproject.rs:76-94`
- **Defect:** proj4rs validates *latitude* (NaN/inf lat → `LatitudeOutOfRange`
  error, confirmed) and errors on non-finite input for *projected* sources
  (`InverseProjectionFailure`, confirmed), but for geographic sources a NaN
  longitude or NaN height passes straight through and `to_ecef` returns
  `Ok([NaN, NaN, …])`:
  - `to_ecef([NaN, 45.0, 0.0])` → `Ok([NaN, NaN, 4487348.408865919])`
  - `to_ecef([10.0, 45.0, NaN])` → `Ok([NaN, NaN, NaN])`
- **Failure scenario:** a point cloud or mesh in EPSG:4326/4979 with a few
  NaN coordinates (common in PLY/E57 exports with invalid points) silently
  becomes NaN ECEF. Downstream, one NaN vertex poisons bbox/centroid
  computation and thus the whole tileset — the exact silent-corruption class
  this crate exists to prevent. Error behavior is also inconsistent across
  source kinds (projected errors, geographic doesn't), so consumers cannot
  rely on `Err` as the corrupt-input signal.
- **Fix direction:** after `transform`, check the three outputs with
  `is_finite()` and return `CrsError::Reproject` naming the input triple
  (mirrors the existing error text). Three comparisons per point is noise
  next to the transform cost. Decide explicitly whether the identity
  (EPSG:4978) path keeps its bit-equal pass-through for non-finite input
  (`to_ecef([NaN,0,0])` → `Ok([NaN,0,0])`, confirmed) — that one is arguably
  by design (documented bit-equal semantics); if kept, document it on
  `to_ecef`.
- **Effort:** 1 file (`reproject.rs`) + unit tests; trivial to test (assert
  `Err` for each non-finite axis, both source kinds).

## Edge cases

### F2 — `ecef_to_geodetic_lonlat` / `EnuFrame::from_ecef_origin` silently accept degenerate origins (geocenter, deep-interior points) — **confirmed**

- **Refs:** `src/enu.rs:29-42`, `src/enu.rs:63-79`
- **Defect:** proj4rs's geocentric inverse does not reject points at or near
  the Earth's center; it returns a "valid" answer:
  - `ecef_to_geodetic_lonlat([0,0,0])` → `Ok((0.0, 1.5707963…))` — the
    **north pole**.
  - `ecef_to_geodetic_lonlat([1,1,1])` → `Ok((0.785…, 2.34e-5))`.
  - `EnuFrame::from_ecef_origin([0,0,0])` → `Ok` with a polar-tangent frame.
- **Failure scenario:** an upstream bug (all-invalid points → zeroed
  centroid, or an ENU/local-frame value accidentally fed in as ECEF) produces
  a tileset root transform anchored at the north pole with no error anywhere.
  The output renders — in the wrong place — which is the hardest failure mode
  to diagnose downstream.
- **Fix direction:** in `from_ecef_origin` (and/or `ecef_to_geodetic_lonlat`),
  reject origins whose radius `|ecef|` is implausible for a georeferenced
  dataset — e.g. below ~6.2e6 m (well under Earth's polar radius minus the
  Mariana Trench) — with a `CrsError::Reproject` naming the radius. Also
  covers NaN input (`ecef_to_geodetic_lonlat([NaN,0,0])` → `Ok((NaN, NaN))`,
  confirmed) if the check is written as `!(r >= MIN_R)`.
- **Effort:** 1 file (`enu.rs`) + unit tests; easy to test.

### F3 — GeoTIFF sentinel codes 0 and 32767 parse as "valid" EPSG through both the short form and WKT — **confirmed (short form), code-read (WKT)**

- **Refs:** `src/sidecar.rs:137-143` (`parse_short_form_epsg`),
  `src/wkt.rs:292-297` (`parse_authority_args`), contrast
  `src/reproject.rs:104-115` (`is_geotiff_sentinel` docs: "never feed it to
  `Reprojector::new`").
- **Defect:** `parse_crs_string("EPSG:0")` →
  `Ok(ParsedCrs { epsg: 0, … })` and `"EPSG:32767"` → `Ok(32767)`
  (confirmed). The WKT path parses `AUTHORITY["EPSG","0"]` /
  `ID["EPSG",32767]` the same way (code-read: `"0".parse::<u16>()` succeeds).
  The crate's own `is_geotiff_sentinel` doc says these codes mean *absence of
  a CRS*, yet the parsers hand them to callers as detected EPSGs.
- **Failure scenario:** a broken export pipeline writes `EPSG:0` into a
  `.prj`; the sidecar resolves to `Detected(0)`; `Reprojector::new` then
  fails with the misleading "EPSG:0 not in crs-definitions catalogue" instead
  of either a clean "sentinel = no CRS declared" error or sentinel-as-absence
  handling. Every consumer must independently remember to call
  `is_geotiff_sentinel` between parse and use.
- **Fix direction:** reject sentinels inside `parse_short_form_epsg` and
  `parse_authority_args` with a `CrsError::Parse` that says the code is a
  GeoTIFF undefined/user-defined sentinel, not a CRS (keeps the crate's
  "malformed sidecar errors loudly" philosophy and centralizes the check).
  Minor related nit: `"EPSG:+123"` parses (Rust `u16::parse` accepts a
  leading `+`, confirmed) — harmless, tighten only if rejecting sentinels
  anyway.
- **Effort:** 2 files (`sidecar.rs`, `wkt.rs`) + tests; easy to test. Check
  consumers don't currently *depend* on sentinel pass-through before landing.

## Performance

### F4 — `ecef_to_geodetic_lonlat` constructs two `Proj` objects on every call — **confirmed, minor**

- **Refs:** `src/enu.rs:29-42`
- **Measured:** ~1.0 µs/call (release build, probe over 10k iterations),
  dominated by `Proj::from_epsg_code` parsing the proj-string catalogue
  entries each time.
- **Failure scenario:** none today — the only in-crate caller is
  `EnuFrame::from_ecef_origin` (once per dataset). But the function is `pub`
  and exported at the crate root; a consumer calling it per-point on a 100M
  point cloud would pay ~100 s of pure re-parsing.
- **Fix direction:** hoist the two `Proj` values into
  `std::sync::LazyLock` statics (EPSG:4978 and 4326 are fixed), or document
  the per-call construction cost on the function.
- **Effort:** 1 file, trivial; existing tests already cover the path.

## Robustness & hygiene

### F5 — Axis-order and unit contract of `to_ecef` for geographic sources is undocumented (lon/lat-swap hazard) — **confirmed behavior, doc gap**

- **Refs:** `src/reproject.rs:71-83` (`to_ecef` doc + degrees→radians
  branch), `src/lib.rs:10-12`
- **Defect:** for geographic sources `to_ecef` requires
  `x = longitude (deg East)`, `y = latitude (deg North)` — proj4 GIS order.
  Neither the method doc nor the crate doc states this. EPSG:4326's
  *official* axis order is lat,lon (the crate's own WKT2 test fixture at
  `src/wkt.rs:444-446` shows `AXIS["…latitude…"]` first), so a careful caller
  honoring EPSG order gets silently wrong output. The swap only surfaces as
  an error when |lat-as-lon| > 90 (confirmed: `LatitudeOutOfRange`); for
  e.g. (lon 45, lat 12) swapped to (12, 45) the output is plausible-looking
  garbage a continent away.
- **Failure scenario:** a new consumer (imagery, optimize, future readers)
  wires a lat,lon source into `to_ecef` without swapping; every tileset it
  produces is displaced, no error anywhere.
- **Fix direction:** state the contract explicitly on `to_ecef` and in the
  `lib.rs` summary: "geographic sources: `[lon_deg, lat_deg, h_m]`
  (GIS/proj4 order), NOT the EPSG-official lat,lon order"; add the existing
  round-trip unit test values as a doc example. A `debug_assert!` that
  |y| ≤ 90 for lat-long sources is a cheap extra tripwire (|x| ≤ 180 would
  false-positive on wrapped longitudes, which proj4rs handles — confirmed).
- **Effort:** 1 file, docs-only (plus optional debug assert); zero test risk.

### F6 — Sidecar suffix doc self-contradicts, and uppercase `.PRJ` is silently ignored — **code-read**

- **Refs:** `src/sidecar.rs:62-70` (doc), `src/sidecar.rs:32`,
  `src/sidecar.rs:67-71` (lowercase-only `with_extension`)
- **Defect:** the doc says "The lookup is case-insensitive in the suffix
  only" then gives an example showing it is *not* found — and the code tries
  only lowercase `.prj`/`.qpj`. A `FOO.PRJ` or `foo.PRJ` sidecar (common from
  legacy ESRI/Windows tooling) is not tried; the caller gets `Ok(None)` and
  falls back to local-frame — silent loss of georeferencing, which is exactly
  the failure mode the module doc says must not be silent ("a malformed
  `.prj` next to a PLY is almost always a user mistake").
- **Fix direction:** decide and align: either (a) also probe uppercase
  variants (`.PRJ`, `.QPJ`) — 2 extra `is_file` calls — or (b) keep
  lowercase-only and rewrite the doc sentence to say the lookup is
  case-sensitive lowercase by design. (a) matches the loud-failure
  philosophy better.
- **Effort:** 1 file + 1 test; easy to test with a tempdir.

### F7 — Stale comment: `parity_oracle.rs` claims the `proj` crate "stays in `[dev-dependencies]`", but Cargo.toml has no dev-dependencies at all — **confirmed**

- **Refs:** `tests/parity_oracle.rs:24-26`, `Cargo.toml` (deps: only
  `proj4rs` + `thiserror`)
- **Defect:** doc/manifest drift. Harmless today, but a future agent reading
  the oracle's rationale will go looking for a manifest entry that does not
  exist and may "restore" the FFI dependency the Cargo.toml NOTE explicitly
  forbids (build hosts lack libproj/SQLite3).
- **Fix direction:** delete or reword the sentence in the test-file comment
  to match the Cargo.toml NOTE (frozen fixtures only, no `proj` dep anywhere).
- **Effort:** 1 file, comment-only.

## Missing tests

The suite is genuinely good for its size (55 tests, frozen dual-hemisphere
PROJ parity grids, WKT1+WKT2 corners, sidecar precedence). Gaps, in order of
the risk they leave open:

### MT1 — No frozen PROJ parity fixture for a *geographic* source

`tests/parity_oracle.rs` pins only projected sources (EPSG:32617, 32719). The
degrees→radians branch (`reproject.rs:82-83`, the ADR-041 C1 fix) is guarded
only by unit tests that use proj4rs to check proj4rs. Probe result: the
branch currently matches closed-form WGS84 to 0.0 m, so nothing is wrong —
but a proj4rs version bump that regressed only the lat-long path would slip
past the oracle. Fix: add a third 27-point grid for EPSG:4326 (and ideally
EPSG:4979) captured from cs2cs, same format. 2 files (fixture + test fn);
requires a PROJ-equipped machine once, per the Cargo.toml NOTE.

### MT2 — No coverage of non-metre projected units

No test exercises a US-survey-feet or foot CRS (state planes are common in
North American lidar). Probe confirmed EPSG:2926 converts correctly today via
proj4rs `+units=us-ft` handling; a pinned single-point test (reproject.rs
style) would lock that in. 1 file.

### MT3 — Non-finite input behavior is untested

Ties to F1: whatever policy is chosen, each axis × {NaN, ±inf} ×
{geographic, projected, identity} should have asserted behavior. Currently
zero tests mention NaN. 1 file.

### MT4 — `EnuFrame` tested at a single mid-northern-latitude point

All four ENU tests use the Žilina sample (lat 49°N). Poles, southern/western
hemispheres, and the antimeridian are untested. Probe swept lat −89.9..89.9 ×
7 longitudes: worst orthonormality deviation 2.2e-16, so the math is fine —
add a small loop over extreme anchors to keep it that way. Also:
`ecef_to_geodetic_lonlat` has no direct known-value test (only usage-based
coverage); one assert against a published point would cover it. 1 file.

## Verification appendix (probe outputs)

Probe crate: scratchpad `crs-probe` (not in repo), release build against this
commit. Key raw results backing the confirmed findings:

```
to_ecef 4326 (0,0,0)      -> Ok([6378137.0, 0.0, 0.0])            # exact a
to_ecef 4326 (0,90,0)     -> Ok([3.9e-10, 0.0, 6356752.314245179]) # exact b
to_ecef 4326 (0,100,0)    -> Err(LatitudeOutOfRange)               # clean
to_ecef 4326 (NaN,45,0)   -> Ok([NaN, NaN, 4487348.408865919])     # F1
to_ecef 4326 (10,45,NaN)  -> Ok([NaN, NaN, NaN])                   # F1
to_ecef 32617 (NaN,…)     -> Err(InverseProjectionFailure)         # contrast
ecef_to_geodetic([0,0,0]) -> Ok((0.0, 1.5707963267948966))         # F2: pole
EnuFrame::from_ecef_origin([0,0,0]) -> Ok(polar frame)             # F2
parse_crs_string("EPSG:0")     -> Ok(ParsedCrs { epsg: 0, .. })    # F3
parse_crs_string("EPSG:32767") -> Ok(ParsedCrs { epsg: 32767, .. })# F3
EPSG:2926 usft (1266000,230000,0) -> lon -122.3516 lat 47.6204     # MT2 ok
ecef_to_geodetic_lonlat timing: ~0.99 us/call                      # F4
ENU orthonormality sweep worst: 2.2e-16; 50 km round-trip 3.2e-10 m
closed-form WGS84 deltas at 4 extreme points: 0.0 m                # MT1 ok
```
