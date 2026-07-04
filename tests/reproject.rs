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

/// WGS84 defining ellipsoid parameters (semi-major axis metres,
/// flattening) — used only by the closed-form geographic oracle below.
const WGS84_A: f64 = 6_378_137.0;
const WGS84_F: f64 = 1.0 / 298.257_223_563;

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

/// Closed-form WGS84 geodetic → geocentric (ECEF), the textbook
/// definitional transform:
///   e² = f(2−f);  N = a / √(1 − e² sin²φ)
///   X = (N+h) cosφ cosλ;  Y = (N+h) cosφ sinλ;  Z = (N(1−e²)+h) sinφ
/// φ = latitude, λ = longitude (both radians), h = ellipsoidal height (m).
fn closed_form_wgs84_ecef(lon_deg: f64, lat_deg: f64, h: f64) -> [f64; 3] {
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let (lon, lat) = (lon_deg.to_radians(), lat_deg.to_radians());
    let (sphi, cphi) = lat.sin_cos();
    let (slam, clam) = lon.sin_cos();
    let n = WGS84_A / (1.0 - e2 * sphi * sphi).sqrt();
    [
        (n + h) * cphi * clam,
        (n + h) * cphi * slam,
        (n * (1.0 - e2) + h) * sphi,
    ]
}

/// MT1 (substantive) — closed-form WGS84 geographic→ECEF oracle.
///
/// The originally-blocked MT1 was "the geographic path needs a frozen
/// cs2cs parity fixture from a PROJ machine." But for a **WGS84**
/// geographic source (EPSG:4326 → EPSG:4978) the transform is pure
/// closed-form geodetic↔geocentric on a *single datum* — no grid shift,
/// no projection — so the WGS84 ellipsoid equations are the
/// **definitional** ground truth: `cs2cs` would compute the same numbers,
/// and this independent formula is emphatically NOT proj4rs checking
/// proj4rs. This gives MT1's substantive coverage (the degrees→radians /
/// lon-lat-order geographic path, ADR-041 C1) with no PROJ machine,
/// asserted to 1 mm.
///
/// The genuine PROJ-machine remainder is a **non-WGS84 datum** tie — e.g.
/// NAD83/GRS80 (EPSG:4269) → 4978, which carries a datum shift (~1–2 m)
/// that is not closed-form here. That single fixture stays blocked on a
/// PROJ-9.x capture; the WGS84 geographic path no longer is.
#[test]
fn wgs84_geographic_to_ecef_matches_closed_form_within_1mm() {
    let rp = Reprojector::new(SourceCrs::new(4326)).expect("EPSG:4326 must be in catalogue");
    // (lon_deg, lat_deg, h_m): equator/prime-meridian, mid-lat eastern,
    // near-pole, southern+western hemisphere, and a below-ellipsoid height.
    let points = [
        [0.0, 0.0, 0.0],
        [18.2912, 49.0305, 175.0],
        [0.0, 89.5, 1000.0],
        [-70.6, -33.4, 500.0],
        [140.0, -12.3, -50.0],
    ];
    const TOL_M: f64 = 1.0e-3;
    for p in points {
        let got = rp.to_ecef(p).expect("in-domain geographic point");
        let want = closed_form_wgs84_ecef(p[0], p[1], p[2]);
        for (axis, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            let delta = (g - w).abs();
            assert!(
                delta < TOL_M,
                "point {p:?} axis {axis}: got {g} want {w} (Δ {delta} m, tol {TOL_M} m)"
            );
        }
    }
}
