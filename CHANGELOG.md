# Changelog

All notable changes to `tileforge-crs` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Downstream crates (`tileforge-pc`, `tileforge-mesh`) consume this crate by git
revision, not by version, so a version bump here does not by itself break them.

## [Unreleased]

### Added
- `Reprojector::datum_shift` and `DatumShift` report the Helmert operation
  that a reprojector applies (identifier, name, published accuracy,
  parameters), so that a consumer can warn the user.
- `CrsError::DatumShiftUnavailable(u16)`: `Reprojector::new` refuses a source
  CRS on an ellipsoid other than GRS80 or WGS 84 when no verified Helmert
  transformation to WGS 84 is available for it: the PROJ database has no
  static one (for example Xian 1980 and New Beijing), or `proj4rs` cannot
  reproduce the PROJ position (`tools/datum-shift-exclusions.txt`). This
  refuses 529 catalogue CRSs that `Reprojector::new` accepted before.
  `is_supported_epsg` returns `false` for these codes. The new variant breaks
  an exhaustive `match` on `CrsError`; a consumer that maps the error through
  `to_string`, `matches!` or a `match` with a wildcard arm is unaffected.
- `tools/gen_datum_shifts.py` generates `src/datum_shift_table.rs` and the
  PROJ reference fixture `tests/fixtures/datum-shift-reference.txt`.
- A compound CRS whose vertical leg is a `BOUNDCRS` on a vertical CRS (`VERTCRS`,
  `VERTICALCRS` or `VERT_CS` as the first child of `SOURCECRS`) parses. PROJ-based
  tools write a geoid model this way. The result is the horizontal EPSG code with
  `vertical_stripped = true`, as for a plain vertical leg; the operation of that
  leg is not read. A `BOUNDCRS` on a horizontal CRS, a `BOUNDCRS` without one
  readable `SOURCECRS`, and `TOWGS84` outside that leg are still errors. So is a
  vertical `BOUNDCRS` that is nested inside a horizontal CRS: only a direct child
  of the compound root is a vertical leg. A vertical `BOUNDCRS` at the root is an
  error with its own message.
- `Reprojector::check_area_of_use` compares one source coordinate (the dataset
  centre) with the EPSG area of use of the source CRS. It returns
  `AreaOfUseCheck::Inside` within 1 degree of the area, `AreaOfUseCheck::Outside`
  (a warning for the consumer, with the position, the area and the excess) from
  1 to 20 degrees of arc outside, and `CrsError::OutsideAreaOfUse` beyond
  20 degrees. A longitude difference is measured along the parallel.
  It catches coordinates in the wrong unit and many wrong CRS declarations.
  `to_ecef` is unchanged: a consumer must call the check.
- `AreaOfUse`, `AreaOfUseCheck`, `OutsideAreaOfUse` and
  `CrsError::OutsideAreaOfUse(String)`.
- `tools/gen_area_of_use.py` generates `src/area_of_use_table.rs` from the `BBOX`
  of 7,492 catalogue definitions.

### Changed
- **Decision reversal: a zero-valued WKT `TOWGS84` is accepted** when the
  stamped EPSG code is a CRS on the GRS80 or WGS 84 ellipsoid in the catalogue
  (WGS 84, NAD83, ETRS89, GDA94 and similar). The crate applies no datum shift
  to such a CRS, so the clause changes nothing in the output. GDAL 2 and
  tools built on it wrote `TOWGS84[0,0,0,0,0,0,0]` for these systems. The
  2026-09-07 decision (`docs/reviews/2026-09-07-explicit-transforms.md`)
  refused it. The Web Mercator codes that carry the null grid (EPSG:3857,
  3785 and 6871) are accepted too: PROJ and the crate apply no shift to them.
  A zero `TOWGS84` on a CRS on any other ellipsoid or on another sphere, on a
  NAD27 CRS or on a code outside the catalogue is an error, because a zero
  shift cannot be confirmed for it. A `TOWGS84` with a non-zero value stays
  refused. A GRS80 or WGS 84 based CRS that gets a Helmert shift from the
  datum shift table (for example EPSG:2100, GGRS87) is refused too: a zero
  clause contradicts the shift.
- `crs-definitions` 0.4 is now a direct dependency (it was already a
  dependency of `proj4rs`; the version and features are the same).
- The WKT scanner rejects a `[` outside a quoted string that does not open the
  block of an identifier (the identifier and optional ASCII whitespace come
  directly before it), and a `]` that closes no block. Before, the scanner read
  the nested clauses of such a bracket as top-level clauses, so text such as a
  vertical tab or a no-break space before `[` could change the EPSG code. Text
  with an extra `]` after the root block also fails now.
- `detect_crs_from_sidecar` refuses a `.prj`/`.qpj` sidecar larger than 1 MiB
  (1,048,576 bytes) with `CrsError::Io`. The read stops one byte past the limit,
  so a large file no longer takes memory equal to its size.
- The WKT scanner rejects the identifiers `BOUNDCRS` and `TOWGS84` outside a
  quoted string whatever character follows them. Before, it rejected them only
  when ASCII whitespace and a `[` followed.
- The WKT scanner rejects `(` and `)` outside a quoted string. It supports
  square-bracket delimiters only. Quoted names that contain parentheses still
  parse. The rule covers the whole string, so text with a parenthesis after
  the root block also fails.
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
- **Apply the datum shift for source CRSs whose datum is offset from WGS 84.**
  The proj4 catalogue carries no datum shift, so `Reprojector` placed the
  latitude and longitude of such a CRS directly on the WGS 84 ellipsoid. The
  position was wrong by the offset of the datum: 83 m to 166 m for the
  British, Dutch, Swiss, German, Austrian, Belgian and ED50 national grids,
  325 m for the Greek Grid, with a median of 112 m over all affected CRSs.
  `Reprojector` now applies a Helmert transformation: the first static Helmert
  operation in the ranking that PROJ 9.8.1 makes, without grid files, for the
  area of use of the CRS. This changes the output for 1,989 catalogue CRSs
  (1,954 on an ellipsoid other than GRS80 or WGS 84, and 35 on GRS80 or WGS 84
  whose first-ranked PROJ operation is a Helmert shift of more than 5 m) and
  for no other CRS. Every one of them agrees with PROJ 9.8.1 to 10 µm on the
  reference fixture. The output of 175 of them (GSK-2011, a zero-parameter
  operation) moves less than 1 m. Known limit: EPSG:6988 to 6991 (IG05/12 and
  Israeli Grid 05/12) are 78 m from WGS 84 and get no shift.
- For the same 1,989 CRSs, restate what `proj4rs` 0.1.10 misreads in a
  catalogue definition: the `+k_0=` scale factor (ignored), an ellipsoid
  given by its minor axis (wrong flattening in the Transverse Mercator
  series), and a `+pm=` prime meridian (degrees added as radians).
- Restate the same three things for every catalogue CRS that gets no datum
  shift. Of these, only definitions that write the scale factor as `+k_0=`
  change: 176 CRSs (189 in the catalogue, 9 of which `proj4rs` cannot build),
  mostly NAD83 low-distortion projections (Oregon, Montana RMTCRS, Wisconsin
  WISCRS, Kansas KS RCS). At the centre of the area of use the old position
  was up to 196 m from PROJ (EPSG:8325 and 8326, Oregon Mitchell zone),
  39 CRSs more than 1 m, median 0.19 m. Every row of the new PROJ reference
  fixture `tests/fixtures/restatement-reference.txt` (378 rows) now agrees to
  10 µm. The output of every other catalogue CRS is bit-identical. Closes
  issue #7.
- `parse_crs_string`, `detect_crs_from_sidecar` and `extract_epsg_from_wkt` ignore
  one leading UTF-8 byte order mark (U+FEFF), and `parse_crs_string` also ignores
  the whitespace after it. Before, a `.prj` saved by Windows Notepad failed with
  "CRS WKT is empty or has no leading identifier". A byte order mark at any other
  position is not special.
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
