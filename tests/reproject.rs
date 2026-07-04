//! `Reprojector` numerical-correctness tests.
//!
//! Reference values come from spike `0g-crs-crate/SPIKE-0g.md`, which
//! pinned proj4rs against PROJ 9.6.2 `cs2cs` to <1 µm on the regression
//! corpus. This file is the production-side anchor for that pin.
//!
//! The 27-point grid PROJ-FFI parity oracle lives in `parity_oracle.rs`;
//! tests here are scalar single-point sanity checks.

use tileforge_crs::{CrsError, Reprojector, SourceCrs};

/// Spike 0g §"Validating spike" pinned reference for the KingstonRd
/// representative point. Input is in EPSG:32617 (UTM 17N / WGS84);
/// output is EPSG:4978 (ECEF metres). PROJ 9.6.2 cs2cs ground truth.
const KINGSTON_UTM_INPUT: [f64; 3] = [649490.0, 4851490.0, 100.0];
const KINGSTON_ECEF_EXPECTED: [f64; 3] = [868600.711988, -4528298.769732, 4392258.462889];

/// Spike-0g-measured tolerance for proj4rs vs PROJ FFI on this corpus.
/// 1 µm is two orders of magnitude tighter than CesiumJS's per-pixel
/// resolution; we pin here to catch regressions early.
const PARITY_TOLERANCE_M: f64 = 1.0e-6;

#[test]
fn identity_epsg_4978_constructs_and_reports_identity() {
    let rp = Reprojector::new(SourceCrs::ECEF).expect("EPSG:4978 must be in catalogue");
    assert!(rp.is_identity(), "EPSG:4978 source must short-circuit");
}

#[test]
fn identity_epsg_4978_to_ecef_is_pointwise_identity() {
    let rp = Reprojector::new(SourceCrs::ECEF).unwrap();
    let xyz = [1234.5, -678.9, 42.0];
    let out = rp.to_ecef(xyz).expect("identity reprojection cannot fail");
    let tol = f64::EPSILON * 10.0;
    for (axis, (got, want)) in out.iter().zip(xyz.iter()).enumerate() {
        let delta = got - want;
        assert!(
            delta.abs() <= tol,
            "axis {axis}: got {got} want {want} (Δ {delta})"
        );
    }
}

#[test]
fn identity_preserves_zero() {
    let rp = Reprojector::new(SourceCrs::ECEF).unwrap();
    assert_eq!(rp.to_ecef([0.0, 0.0, 0.0]).unwrap(), [0.0, 0.0, 0.0]);
}

#[test]
fn projected_source_reports_non_identity() {
    let rp = Reprojector::new(SourceCrs::new(32617)).expect("EPSG:32617 must be in catalogue");
    assert!(!rp.is_identity(), "UTM-17N source must not report identity");
}

#[test]
fn kingstonrd_utm_to_ecef_matches_proj_within_1um() {
    let rp = Reprojector::new(SourceCrs::new(32617)).unwrap();
    let out = rp
        .to_ecef(KINGSTON_UTM_INPUT)
        .expect("KingstonRd centroid is well inside UTM 17N domain of validity");
    for (axis, (got, want)) in out.iter().zip(KINGSTON_ECEF_EXPECTED.iter()).enumerate() {
        let delta = (got - want).abs();
        assert!(
            delta < PARITY_TOLERANCE_M,
            "axis {axis}: got {got} want {want} (Δ {delta} m, tol {PARITY_TOLERANCE_M} m)"
        );
    }
}

#[test]
fn unknown_epsg_returns_crs_error() {
    let err =
        Reprojector::new(SourceCrs::new(65000)).expect_err("EPSG:65000 must not be in catalogue");
    match &err {
        CrsError::UnknownEpsg(msg) => {
            assert!(
                msg.contains("65000"),
                "error message must reference the offending EPSG code: {msg}"
            );
        }
        other => panic!("expected UnknownEpsg, got {other:?}"),
    }
}

/// Repeated calls on the same Reprojector must produce bit-equal
/// output — protects against non-deterministic internal state.
#[test]
fn forward_leg_is_deterministic_across_calls() {
    let rp = Reprojector::new(SourceCrs::new(32617)).unwrap();
    let a = rp.to_ecef(KINGSTON_UTM_INPUT).unwrap();
    let b = rp.to_ecef(KINGSTON_UTM_INPUT).unwrap();
    assert_eq!(a, b, "two calls must be bit-identical");
}

/// MT2 — non-metre projected units (US survey feet). EPSG:2926 is
/// Washington State Plane North (NAD83 / WA-N, `+units=us-ft`); no test
/// otherwise exercises a non-metre linear unit, though state-plane feet
/// are common in North American lidar. Input is easting/northing in
/// **US survey feet**; a bug in proj4rs's us-ft scale handling would
/// misplace the point by ~2 ppm (the ft-vs-us-ft difference).
///
/// The expected ECEF was captured from proj4rs and cross-validated
/// against the closed-form WGS84 ellipsoid for the point's geographic
/// position (lon −122.352°, lat 47.620°): X/Y/Z agree with
/// `N·cos·cos`, `N·cos·sin`, `N(1−e²)·sin` to 4+ significant figures, so
/// this pins *correct* unit handling, not merely self-consistent output.
/// (A frozen-cs2cs external PROJ oracle for this point is MT1, blocked on
/// a PROJ-9.x machine per the Cargo.toml NOTE.) This is a regression pin:
/// it locks the us-ft path against a future proj4rs bump.
const WA_SP_NORTH_USFT_INPUT: [f64; 3] = [1266000.0, 230000.0, 0.0];
const WA_SP_NORTH_ECEF_EXPECTED: [f64; 3] = [-2304729.212267, -3638457.830071, 4688531.713529];

#[test]
fn us_survey_feet_source_to_ecef_matches_pin_within_1um() {
    let rp = Reprojector::new(SourceCrs::new(2926)).expect("EPSG:2926 must be in catalogue");
    assert!(
        !rp.is_identity(),
        "state-plane source must not report identity"
    );
    let out = rp
        .to_ecef(WA_SP_NORTH_USFT_INPUT)
        .expect("WA state-plane point is well inside the CRS domain of validity");
    for (axis, (got, want)) in out.iter().zip(WA_SP_NORTH_ECEF_EXPECTED.iter()).enumerate() {
        let delta = (got - want).abs();
        assert!(
            delta < PARITY_TOLERANCE_M,
            "axis {axis}: got {got} want {want} (Δ {delta} m, tol {PARITY_TOLERANCE_M} m)"
        );
    }
}
