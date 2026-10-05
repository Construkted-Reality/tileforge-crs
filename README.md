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
- `parse_crs_string` rejects the GeoTIFF sentinel codes `EPSG:0` and `EPSG:32767`.
  A sidecar that declares a sentinel is a valid "no CRS" declaration, so
  `detect_crs_from_sidecar` returns it and leaves the policy to the caller.
- Vertical-datum policy belongs to the consumer. The parser only reports `vertical_stripped`.

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

## Documentation

Start at [docs/README.md](docs/README.md). Release history is in [CHANGELOG.md](CHANGELOG.md).
