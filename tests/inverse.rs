// ABOUTME: Checks inverse source coordinates against independent Earth-coordinate controls.
// ABOUTME: Pins geographic axis units, projected units, finite errors, and identity bits.
use tileforge_crs::{CrsError, Reprojector, SourceCrs};

fn wgs84_ecef([longitude, latitude, height]: [f64; 3]) -> [f64; 3] {
    let flattening = 1.0 / 298.257_223_563;
    let eccentricity_squared = flattening * (2.0 - flattening);
    let latitude = latitude.to_radians();
    let longitude = longitude.to_radians();
    let radius = 6_378_137.0 / (1.0 - eccentricity_squared * latitude.sin().powi(2)).sqrt();
    [
        (radius + height) * latitude.cos() * longitude.cos(),
        (radius + height) * latitude.cos() * longitude.sin(),
        (radius * (1.0 - eccentricity_squared) + height) * latitude.sin(),
    ]
}

#[test]
fn geographic_inverse_returns_longitude_latitude_degrees_and_height_metres() {
    let reprojector = Reprojector::new(SourceCrs::new(4326)).unwrap();
    for expected in [
        [-79.4171, 43.64066, 123.456789],
        [18.29, 49.03, 172.25],
        [120.125, -35.25, -12.5],
        [0.0, 0.0, 0.0],
    ] {
        let actual = reprojector.from_ecef(wgs84_ecef(expected)).unwrap();
        for axis in 0..3 {
            let tolerance = if axis == 2 { 1e-6 } else { 1e-10 };
            assert!(
                (actual[axis] - expected[axis]).abs() < tolerance,
                "{actual:?}"
            );
        }
    }
}

#[test]
fn projected_inverse_matches_the_frozen_proj_control() {
    let reprojector = Reprojector::new(SourceCrs::new(32617)).unwrap();
    // Existing PROJ control from tests/reproject.rs, not a forward round trip.
    let actual = reprojector
        .from_ecef([868600.711988, -4528298.769732, 4392258.462889])
        .unwrap();
    for (actual, expected) in actual.into_iter().zip([649490.0, 4851490.0, 100.0]) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }
}

#[test]
fn inverse_keeps_horizontal_survey_feet_separate_from_vertical_metres() {
    let reprojector = Reprojector::new(SourceCrs::new(2926)).unwrap();
    let actual = reprojector
        .from_ecef([-2304729.212267, -3638457.830071, 4688531.713529])
        .unwrap();
    for (actual, expected) in actual.into_iter().zip([1266000.0, 230000.0, 0.0]) {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
    let source = [1266000.0, 230000.0, 123.456];
    let earth = reprojector.to_ecef(source).unwrap();
    let recovered = reprojector.from_ecef(earth).unwrap();
    for axis in 0..3 {
        assert!((recovered[axis] - source[axis]).abs() < 1e-5);
    }
}

#[test]
fn identity_inverse_preserves_finite_coordinate_bits() {
    let reprojector = Reprojector::new(SourceCrs::ECEF).unwrap();
    for input in [[-0.0, 0.0, 1e-300], [1e300, -1e300, 42.123456789]] {
        let output = reprojector.from_ecef(input).unwrap();
        assert_eq!(output.map(f64::to_bits), input.map(f64::to_bits));
    }
}

#[test]
fn inverse_rejects_nonfinite_coordinates_for_every_source_kind() {
    for epsg in [4978, 4326, 32617, 2926] {
        let reprojector = Reprojector::new(SourceCrs::new(epsg)).unwrap();
        for axis in 0..3 {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut input = [100., 200., 300.];
                input[axis] = value;
                assert!(matches!(
                    reprojector.from_ecef(input),
                    Err(CrsError::Reproject(_))
                ));
            }
        }
    }
}
