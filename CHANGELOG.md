# Changelog

All notable changes to `tileforge-crs` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Downstream crates (`tileforge-pc`, `tileforge-mesh`) consume this crate by git
revision, not by version, so a version bump here does not by itself break them.

## [Unreleased]

### Changed
- `detect_crs_from_sidecar` refuses a `.prj`/`.qpj` sidecar larger than 1 MiB
  (1,048,576 bytes) with `CrsError::Io`. The read stops one byte past the limit,
  so a large file no longer takes memory equal to its size.
- The WKT scanner rejects the identifiers `BOUNDCRS` and `TOWGS84` outside a
  quoted string whatever character follows them. Before, it rejected them only
  when ASCII whitespace and a `[` followed.
- The WKT scanner rejects `(` and `)` outside a quoted string. It supports
  square-bracket delimiters only. Quoted names that contain parentheses still
  parse.
- The WKT scanner reads every depth-0 `AUTHORITY`/`ID` clause of a block, not only
  the first one. Two EPSG clauses with different codes are an error; a repeated
  code is accepted. When an EPSG clause exists, clauses of other authorities are
  ignored, so a non-EPSG clause before the EPSG clause no longer causes an error.
  A clause whose arguments cannot be read is an error even after a valid EPSG
  clause.
- A compound CRS (`COMPD_CS`/`COMPOUNDCRS`) with more than one horizontal block is
  an error. Before, the first horizontal block was used.
- Error messages from the WKT scanner say "CRS WKT" instead of "WKT VLR", because
  the scanner also reads sidecars and E57 metadata. The message for a WKT without
  an EPSG `AUTHORITY`/`ID` clause no longer recommends `pdal translate`; it says
  that ESRI-style `.prj` files omit the clause and that the parser resolves a CRS
  by its EPSG code only.
- The combined `parse_crs_string` error names both the WKT1 and the WKT2 root
  keywords.

### Fixed
- Error messages repeat at most 64 characters of caller input and mark a cut with
  `...`. Before, an input token was repeated in full.
- A body that starts with `EPSG:` but carries an invalid code (`EPSG:65536`,
  `EPSG:`, `EPSG:32617 extra`) gets a short-form error that shows the code text.
  Before, it got a WKT error about a missing `[`.
- Rustdoc: `extract_epsg_from_wkt` returns the sentinel codes 0 and 32767 as `Ok`;
  `detect_crs_from_sidecar` uses the first sidecar that exists and does not try the
  later suffixes when it fails; the `f32` spacing of local ENU coordinates (0.98 mm
  at 10 km, 7.8 mm at 100 km, 62.5 mm at 1000 km); the crate logs nothing when an
  override displaces a detected EPSG; stale statements in `error.rs` and
  `source_crs.rs`.

### Removed
- The unused test fixtures `tests/fixtures/fabregualta.epsg.txt` and
  `tests/fixtures/kingston-rd.epsg.txt`.

## [0.3.0] - 2026-09-30 - Expose coordinate units and reject unsupported operations

This release exposes horizontal and vertical coordinate units separately. It rejects explicit datum operations that the converter cannot preserve and rejects reserved GeoTIFF sentinel codes.

### Added
- Expose the selected source CRS (`source_crs`) for consumers that must select a coordinate-bound policy.

- Expose separate horizontal and vertical metre conversion factors (`horizontal_meters_per_unit`, `vertical_meters_per_unit`). Geographic horizontal coordinates return no linear factor.

- **`CrsError::SentinelCode(u16)`** — a CRS string parsed to a GeoTIFF reserved
  sentinel. New variant; existing consumers map `CrsError` via `to_string` / a
  non-exhaustive `matches!` and are unaffected.

### Fixed
- Correct projected height documentation. Horizontal feet do not imply vertical feet; reprojection keeps the catalogue vertical unit.

### Changed
- Reject WKT BOUNDCRS and TOWGS84 definitions instead of discarding their explicit datum transformation. Reproject such data with the declared operation before conversion.

- **`parse_crs_string` / `parse_crs_string_epsg` now reject the GeoTIFF reserved
  sentinels `EPSG:0` and `EPSG:32767`.** Per the GeoTIFF spec these are not EPSG
  codes but a declaration of "no standard CRS"; the parser previously handed them
  back as `Ok(0)` / `Ok(32767)`, pushing the not-a-real-code decision onto every
  caller. They are now rejected with the new structured `CrsError::SentinelCode(u16)`
  variant (carrying the sentinel value). The private/user range `32768..` is
  unaffected and still parses. The exported
  [`is_geotiff_sentinel`] helper remains available for consumers that need to
  detect-then-handle a sentinel as absence (fall through to a local frame).
  (Contract tightening: previously-`Ok` sentinel input is now `Err`. Finding F3.)
  `detect_crs_from_sidecar` is unchanged: a `.prj`/`.qpj` declaring a sentinel is
  a valid "no CRS" declaration, so it still resolves to `Ok(Some(SidecarCrs { epsg:
  0 | 32767, .. }))` and leaves the absence policy to the caller — it does **not**
  become a parse error.

## [0.2.0] - 2026-07-04

Review-remediation release. Hardens the error surface so invalid input fails
loudly instead of silently producing wrong coordinates, plus documentation,
performance, and test-coverage fixes from the 2026-07-02 comprehensive review.

### Changed

- **`Reprojector::to_ecef` now rejects non-finite input.** A `NaN`/`±inf`
  longitude, latitude, or height previously passed through for geographic (and
  identity) sources and returned `Ok([NaN, …])`; it now returns
  `CrsError::Reproject` naming the offending triple. One poisoned vertex can no
  longer silently corrupt a whole tileset's bbox/centroid. (Contract tightening:
  previously-`Ok` invalid input is now `Err`.)
- **`EnuFrame::from_ecef_origin` / `ecef_to_geodetic_lonlat` now reject
  implausible ECEF origins.** Non-finite origins, and any origin whose radius
  falls outside `[6.2e6, 6.6e6]` m, are rejected with an actionable
  `CrsError::Reproject`. This catches the geocenter / deep-interior case (which
  used to anchor a tileset at the north pole) and gross scale/unit errors (e.g. a
  ×1000 mm-as-metres mistake). (Contract tightening: previously-`Ok` invalid
  input is now `Err`.)
- Documented the geographic axis-order / unit contract on `to_ecef` and in the
  crate summary: geographic sources take `[lon_deg, lat_deg, h_m]` (GIS/proj4
  order), **not** the EPSG-official lat,lon order. Added a debug-only
  lon/lat-swap tripwire (`debug_assert!`); release behaviour is unchanged.

### Fixed

- Sidecar CRS lookup now finds uppercase `.PRJ` / `.QPJ` sidecars. Previously
  only lowercase `.prj` / `.qpj` were probed, so a legacy ESRI/Windows sidecar
  on a case-sensitive filesystem silently dropped georeferencing.
- Corrected drifted or self-contradictory doc comments (sidecar suffix lookup,
  the `to_ecef` contract, and a stale parity-oracle dev-dependency note).

### Performance

- `ecef_to_geodetic_lonlat` hoists its two `Proj` objects into `LazyLock`
  statics instead of re-parsing the proj4 catalogue (~1 µs) on every call —
  relevant for consumers that call it per point.

### Tests

- Added a closed-form WGS84 geographic→ECEF oracle (independent of proj4rs), a
  US-survey-feet projected-unit pin (EPSG:2926), non-finite-input rejection
  coverage, an extreme-anchor ENU orthonormality sweep, and latitude-range /
  pole-boundary pins.

### Deferred

- **F3 — GeoTIFF sentinel codes `0` / `32767`.** Not landed in this release:
  `tileforge-pc` relies on `parse_crs_string("EPSG:0") -> Ok(0)` and calls
  `is_geotiff_sentinel` itself, so rejecting sentinels inside the parser is a
  cross-consumer contract change pending sign-off. Detect them with the
  exported `is_geotiff_sentinel` helper.

## [0.1.0]

- Initial extraction of the error-agnostic CRS core from `tileforge-pc-crs`
  (tileforge-pc ADR-004 / tileforge-mesh ADR-041 Phase 0): `SourceCrs`,
  `Reprojector` (proj4rs → EPSG:4978 ECEF), WKT1/WKT2 and `EPSG:NNNNN` sidecar
  parsing, `EnuFrame` local-frame math, and the `is_geotiff_sentinel` /
  `is_supported_epsg` / `is_geographic_epsg` helpers.
