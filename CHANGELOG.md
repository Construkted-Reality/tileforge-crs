# Changelog

All notable changes to `tileforge-crs` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Downstream crates (`tileforge-pc`, `tileforge-mesh`) consume this crate by git
revision, not by version, so a version bump here does not by itself break them.

## [Unreleased]

### Fixed
- **Apply the datum shift for source CRSs on an ellipsoid other than GRS80 or
  WGS 84.** The proj4 catalogue carries no datum shift, so `Reprojector`
  placed the latitude and longitude of such a CRS directly on the WGS 84
  ellipsoid. The position was wrong by the offset of the datum: 83 m to
  166 m for the British, Dutch, Swiss, German, Austrian, Belgian and ED50
  national grids, with a median of 111 m over all affected CRSs. `Reprojector`
  now applies the EPSG Helmert transformation that PROJ ranks first, without
  grid files, over the area of use of the CRS. This changes the output for
  1,954 catalogue CRSs and for no other CRS. Every one of them agrees with
  PROJ 9.8.1 to 10 µm on the reference fixture.
- For the same 1,954 CRSs, restate what `proj4rs` 0.1.10 misreads in a
  catalogue definition: the `+k_0=` scale factor (ignored), an ellipsoid
  given by its minor axis (wrong flattening in the Transverse Mercator
  series), and a `+pm=` prime meridian (degrees added as radians).

### Added
- `Reprojector::datum_shift` and `DatumShift` report the Helmert operation
  that a reprojector applies (identifier, name, published accuracy,
  parameters), so that a consumer can warn the user.
- `CrsError::DatumShiftUnavailable(u16)`: `Reprojector::new` refuses a source
  CRS that needs a datum shift when no verified Helmert transformation to
  WGS 84 is available for it (540 catalogue CRSs that `Reprojector::new`
  accepted before, for example Xian 1980 and New Beijing).
  `is_supported_epsg` returns `false` for these codes.
- `tools/gen_datum_shifts.py` generates `src/datum_shift_table.rs` and the
  PROJ reference fixture `tests/fixtures/datum-shift-reference.txt`.

### Changed
- `crs-definitions` 0.4 is now a direct dependency (it was already a
  dependency of `proj4rs`; the version and features are the same).

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
