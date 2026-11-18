// ABOUTME: Checks which source CRSs get a Helmert datum shift, which are refused, and how height is kept.
// ABOUTME: Reference positions come from PROJ 9.8.1 with the EPSG operation named in each test.
//! Datum shift behaviour of `Reprojector`.
//!
//! `datum_shift_parity.rs` pins every CRS of the shift table against PROJ.
//! The tests here pin the contract around the table.

use tileforge_crs::{CrsError, Reprojector, SourceCrs, is_supported_epsg};

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// EPSG:27700 (OSGB36 / British National Grid) at central London, and the
/// EPSG:4978 position that PROJ 9.8.1 gives with "OSGB36 to WGS 84 (6)"
/// (EPSG:1314, published accuracy 2 m).
const LONDON_BNG: [f64; 3] = [530_000.0, 180_000.0, 20.0];
const LONDON_ECEF: [f64; 3] = [
    3_978_303.501_217_941,
    -8_912.205_525_196_3,
    4_968_654.501_432_861,
];

#[test]
fn osgb36_national_grid_is_shifted_to_wgs84() {
    let rp = Reprojector::new(SourceCrs::new(27700)).unwrap();
    let ecef = rp.to_ecef(LONDON_BNG).unwrap();
    let delta = distance(ecef, LONDON_ECEF);
    assert!(delta < 1.0e-5, "{delta} m from the PROJ reference");
}

#[test]
fn datum_shift_names_the_operation_and_its_accuracy() {
    let rp = Reprojector::new(SourceCrs::new(27700)).unwrap();
    let shift = rp.datum_shift().expect("EPSG:27700 needs a datum shift");
    assert_eq!(shift.operation_id, "EPSG:1314");
    assert_eq!(shift.operation_name, "OSGB36 to WGS 84 (6)");
    assert_eq!(shift.accuracy_m, Some(2.0));
    assert_eq!(
        shift.parameters,
        [446.448, -125.157, 542.06, 0.15, 0.247, 0.842, -20.489]
    );
}

#[test]
fn wgs84_and_grs80_sources_get_no_datum_shift() {
    // WGS 84 geographic, WGS 84 UTM, NAD83 state plane, ETRS89 UTM, ECEF.
    for epsg in [4326_u16, 32617, 2926, 25832, 4978] {
        let rp = Reprojector::new(SourceCrs::new(epsg)).unwrap();
        assert_eq!(rp.datum_shift(), None, "EPSG:{epsg}");
    }
}

#[test]
fn datum_without_a_published_shift_is_refused() {
    // EPSG:2327 is Xian 1980 / Gauss-Kruger zone 13. EPSG publishes no
    // transformation from Xian 1980 to WGS 84.
    let err = Reprojector::new(SourceCrs::new(2327)).unwrap_err();
    assert_eq!(err, CrsError::DatumShiftUnavailable(2327));
    let message = err.to_string();
    assert!(message.contains("EPSG:2327"), "{message}");
    assert!(message.contains("WGS 84"), "{message}");
    assert!(!is_supported_epsg(2327));
}

#[test]
fn supported_epsg_agrees_with_reprojector_for_shifted_sources() {
    for epsg in [27700_u16, 28992, 2056, 21781, 31467, 31287, 31370, 23030] {
        assert!(is_supported_epsg(epsg), "EPSG:{epsg}");
        assert!(
            Reprojector::new(SourceCrs::new(epsg)).is_ok(),
            "EPSG:{epsg}"
        );
    }
}

#[test]
fn height_passes_through_the_datum_shift_unchanged() {
    // The shift moves the horizontal position only. Height stays a height
    // above the WGS 84 ellipsoid, so two inputs that differ only in height
    // are that same distance apart along the ellipsoid normal.
    let rp = Reprojector::new(SourceCrs::new(27700)).unwrap();
    let ground = rp.to_ecef([530_000.0, 180_000.0, 0.0]).unwrap();
    let raised = rp.to_ecef([530_000.0, 180_000.0, 1_000.0]).unwrap();
    assert!((distance(ground, raised) - 1_000.0).abs() < 1.0e-6);

    // The ground point lies on the WGS 84 ellipsoid: (x² + y²)/a² + z²/b² = 1.
    let (a, b) = (6_378_137.0_f64, 6_356_752.314_245_179_f64);
    let on_ellipsoid =
        (ground[0].powi(2) + ground[1].powi(2)) / (a * a) + ground[2].powi(2) / (b * b);
    assert!((on_ellipsoid - 1.0).abs() < 1.0e-9, "{on_ellipsoid}");
}

#[test]
fn geographic_source_with_datum_shift_takes_degrees() {
    // EPSG:4277 is geographic OSGB36. Input stays `[lon_deg, lat_deg, h_m]`.
    // The same ground point as LONDON_BNG, in OSGB36 degrees from PROJ.
    let rp = Reprojector::new(SourceCrs::new(4277)).unwrap();
    assert!(rp.datum_shift().is_some());
    let national_grid = Reprojector::new(SourceCrs::new(27700)).unwrap();
    let want = national_grid.to_ecef(LONDON_BNG).unwrap();
    let got = rp
        .to_ecef([-0.126_747_679_945_943_27, 51.503_480_036_972_256, 20.0])
        .unwrap();
    assert!(distance(got, want) < 1.0e-5, "{} m", distance(got, want));
}
