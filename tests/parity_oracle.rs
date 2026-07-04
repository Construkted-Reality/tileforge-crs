//! Reference-oracle parity test: pin proj4rs against PROJ.
//!
//! For each EPSG fixture (`kingston-rd`, `fabregualta`), reproject a
//! 27-point grid (3³ corner+midpoint of the LAS-header bbox) through
//! `proj4rs` and assert each output axis matches the cs2cs-captured
//! ground truth in `tests/fixtures/<name>.parity-grid.txt` to <1 µm
//! absolute.
//!
//! The fixture was captured 2026-05-04 via PROJ 9.8.1 cs2cs. This test
//! is **not** `#[ignore]`d; it runs on every `cargo test`, catching any
//! drift in:
//!
//! - proj4rs version bumps that produce different output;
//! - our EPSG-extraction logic (the source EPSG fed into `Reprojector`);
//! - the `Proj::from_epsg_code` resolution path inside proj4rs.
//!
//! ## Why a frozen fixture and not a runtime PROJ-FFI cross-check
//!
//! The session-plan rev-1 design called for `proj` 0.31 (PROJ FFI) at
//! runtime. That crate's public API hard-codes `z = 0.0` in its 2D
//! `convert` path; full 3D parity through it would require either a
//! shell-out to `cs2cs` or direct proj-sys FFI. Frozen fixture +
//! cs2cs-at-capture-time gives equivalent ground-truth quality with no
//! runtime FFI dependency. The `proj` FFI crate is therefore **not** a
//! dependency anywhere — `Cargo.toml` has no `[dev-dependencies]` and its
//! NOTE forbids adding libproj/SQLite3 (build hosts lack them). Regenerate
//! the fixtures with `cs2cs` on a PROJ-equipped machine if drift is
//! suspected; do not reintroduce the FFI dep.

use std::path::{Path, PathBuf};

use tileforge_crs::{Reprojector, SourceCrs};

/// Spike-0g-measured tolerance for proj4rs vs PROJ. <1 µm catches
/// algorithmic drift; CesiumJS's per-pixel resolution is millions of
/// times looser.
const PARITY_TOLERANCE_M: f64 = 1.0e-6;

struct GridRow {
    src: [f64; 3],
    expected_ecef: [f64; 3],
}

fn parse_grid(name: &str) -> Vec<GridRow> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut rows = Vec::new();
    for (lineno, line) in body.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<f64> = line
            .split_whitespace()
            .map(|t| {
                t.parse::<f64>().unwrap_or_else(|e| {
                    panic!("{}:{}: {t:?} is not f64: {e}", path.display(), lineno + 1)
                })
            })
            .collect();
        assert_eq!(
            cols.len(),
            6,
            "{}:{}: expected 6 fields",
            path.display(),
            lineno + 1
        );
        rows.push(GridRow {
            src: [cols[0], cols[1], cols[2]],
            expected_ecef: [cols[3], cols[4], cols[5]],
        });
    }
    assert_eq!(
        rows.len(),
        27,
        "{}: parity grid must have 27 points",
        path.display()
    );
    rows
}

fn assert_grid_parity(epsg: u16, rows: &[GridRow]) {
    let rp = Reprojector::new(SourceCrs::new(epsg))
        .unwrap_or_else(|e| panic!("EPSG:{epsg} must be in catalogue: {e}"));
    let mut worst = 0.0f64;
    for (i, row) in rows.iter().enumerate() {
        let out = rp.to_ecef(row.src).unwrap_or_else(|e| {
            panic!("EPSG:{epsg} row {i}: reproject({:?}) failed: {e}", row.src)
        });
        for (axis, (got, want)) in out.iter().zip(row.expected_ecef.iter()).enumerate() {
            let delta = (got - want).abs();
            worst = worst.max(delta);
            assert!(
                delta < PARITY_TOLERANCE_M,
                "EPSG:{epsg} row {i} axis {axis}: got {got} want {want} (Δ {delta:e} m, tol {PARITY_TOLERANCE_M:e} m)"
            );
        }
    }
    eprintln!(
        "EPSG:{epsg}: 27/27 grid points within {:e} m of PROJ 9.8.1 (worst Δ {:e} m)",
        PARITY_TOLERANCE_M, worst
    );
}

#[test]
fn proj4rs_matches_proj_for_kingstonrd_grid_within_1um() {
    let rows = parse_grid("kingston-rd.parity-grid.txt");
    assert_grid_parity(32617, &rows);
}

#[test]
fn proj4rs_matches_proj_for_fabregualta_grid_within_1um() {
    let rows = parse_grid("fabregualta.parity-grid.txt");
    assert_grid_parity(32719, &rows);
}
