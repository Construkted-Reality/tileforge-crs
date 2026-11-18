# tileforge-crs

The coordinate reference system (CRS) core shared by `tileforge-mesh` and `tileforge-pc`.
It parses EPSG short forms, OGC WKT, and `.prj`/`.qpj` sidecars into an EPSG code.
It reprojects to ECEF (EPSG:4978) with `proj4rs` and builds the local East-North-Up
frame used as a 3D Tiles root `transform`.

The crate has no dependency on either consumer's error types. It returns `CrsError`,
and each consumer maps that onto its own error schema.

Consumers pin this crate by Git revision in their `Cargo.toml`, not by version.
A release here changes nothing downstream until a consumer moves its pin.

## Contracts that callers get wrong

- Geographic sources take `[lon_deg, lat_deg, h_m]`, not the EPSG-official lat,lon order.
- Invalid input fails with an error. This covers non-finite coordinates, ECEF origins
  outside `[6.2e6, 6.6e6]` m, and WKT `BOUNDCRS`/`TOWGS84` operations.
- The WKT scanner supports square-bracket delimiters only. A `(` or `)` outside a quoted
  string is an error.
- An ambiguous WKT identifier is an error: two different EPSG codes in the top-level
  `AUTHORITY`/`ID` clauses of one block, or a compound CRS with two horizontal blocks.
- The parser resolves a CRS by its EPSG code only. A WKT with no EPSG `AUTHORITY`/`ID`
  clause fails, and ESRI-style `.prj` files usually have none.
- `detect_crs_from_sidecar` uses the first sidecar that exists (`.prj`, `.PRJ`, `.qpj`,
  `.QPJ`). If that file fails to read or parse, the call fails; later suffixes are not tried.
  A sidecar larger than 1 MiB fails with `CrsError::Io`.
- `extract_epsg_from_wkt` returns the sentinel codes 0 and 32767 as `Ok`. Check them with
  `is_geotiff_sentinel`.
- `parse_crs_string` rejects the GeoTIFF sentinel codes `EPSG:0` and `EPSG:32767`.
  A sidecar that declares a sentinel is a valid "no CRS" declaration, so
  `detect_crs_from_sidecar` returns it and leaves the policy to the caller.
- Vertical-datum policy belongs to the consumer. The parser only reports `vertical_stripped`.
- A source CRS whose datum is offset from WGS 84 gets a Helmert datum shift: every CRS on
  an ellipsoid other than GRS80 or WGS 84 (OSGB36, Amersfoort, CH1903, DHDN, MGI, ED50
  and so on), and the few GRS80 or WGS 84 based datums that are more than 5 m from
  WGS 84 (GGRS87, VN-2000, Israel 1993). `Reprojector::datum_shift` reports the operation
  and its published accuracy, typically 1 m to 10 m. A consumer should pass that on as a
  warning. The shift moves the horizontal position only. Height passes through as a
  height above the WGS 84 ellipsoid.
- Other GRS80 or WGS 84 based datums (NAD83, ETRS89, GDA94 and so on) get no shift. They
  are within about 2 m of WGS 84.
- `Reprojector::new` fails with `CrsError::DatumShiftUnavailable` for a CRS on a foreign
  ellipsoid when no verified static Helmert transformation is available: the PROJ
  database has none, or `proj4rs` cannot reproduce the PROJ position
  (`tools/datum-shift-exclusions.txt`).

The rustdoc in `src/lib.rs` is the full API reference.

## Build and test

```sh
cargo test
cargo test --release
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The parity oracle compares against frozen PROJ grids in `tests/fixtures/`.
The `proj` FFI crate is deliberately not a dependency. Regenerate fixtures with `cs2cs`
on a machine that has PROJ installed.

## Datum shift table

`src/datum_shift_table.rs` and `tests/fixtures/datum-shift-reference.txt` are generated.
To regenerate them, for example after an upgrade of `crs-definitions`:

```sh
python3 -m venv venv && venv/bin/pip install pyproj==3.8.0
venv/bin/python tools/gen_datum_shifts.py <path to crs-definitions-0.4.0>/src/defs.rs .
```

The crate source is under `~/.cargo/registry/src/`. Name one directory, not a shell
pattern. `pyproj` 3.8.0 bundles PROJ 9.8.1; another version can rank operations differently.

The tool needs no system PROJ: the `pyproj` wheel bundles PROJ and the EPSG database.
Keep the PROJ network off and install no grid files, so that PROJ ranks only operations
that need no grid. A unit test fails when the table and the catalogue disagree.

`tools/datum-shift-exclusions.txt` lists CRSs that have a Helmert transformation in PROJ
but that `proj4rs` cannot place where PROJ places them. The tool puts them on the refusal
list. After a regeneration, `cargo test --test datum_shift_parity` must pass. If it names
a CRS, add that CRS to the exclusion list with the reason and regenerate.

`proj4rs` 0.1.10 misreads `+k_0=`, an ellipsoid given by its minor axis, and `+pm=`.
`DatumShift::definition_with_shift` restates these for each shifted CRS. Catalogue CRSs
that get no datum shift are not restated; see the open items in
[docs/README.md](docs/README.md).

Limits. A Helmert transformation is an approximation. Where a national grid file exists
(OSTN15, BETA2007 and so on) the Helmert position differs from the grid position by
decimetres to a few metres. The table holds one operation per CRS: the first static Helmert
operation in the ranking that PROJ makes for the area of use of the CRS. That operation
can cover only a part of the area, and its published accuracy holds only there. PROJ can
select a more local operation for a small area, and for some CRSs it ranks an operation of
another kind (Molodensky-Badekas, two steps) first.

## Documentation

Start at [docs/README.md](docs/README.md). Release history is in [CHANGELOG.md](CHANGELOG.md).
