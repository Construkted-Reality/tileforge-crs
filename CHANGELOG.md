# Changelog

All notable changes to `tileforge-crs` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Downstream crates (`tileforge-pc`, `tileforge-mesh`) consume this crate by git
revision, not by version, so a version bump here does not by itself break them.

## [Unreleased]

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
