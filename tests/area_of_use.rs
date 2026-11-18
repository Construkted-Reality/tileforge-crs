// ABOUTME: Checks the area-of-use plausibility check of Reprojector on real CRSs.
// ABOUTME: Covers in-area data, national grids used past their zone, and wrong-unit input.
//! `Reprojector::check_area_of_use` behaviour.

use tileforge_crs::{AreaOfUseCheck, CrsError, Reprojector, SourceCrs};

fn reprojector(epsg: u16) -> Reprojector {
    Reprojector::new(SourceCrs::new(epsg)).unwrap()
}

#[test]
fn coordinate_inside_the_area_of_use_passes() {
    // Kingston Rd, Toronto, in WGS 84 / UTM zone 17N.
    let check = reprojector(32617).check_area_of_use([649_490.0, 4_851_490.0, 100.0]);
    assert_eq!(check, Ok(AreaOfUseCheck::Inside));
    // Seattle in NAD83(HARN) / Washington North (US survey feet).
    let check = reprojector(2926).check_area_of_use([1_270_000.0, 230_000.0, 50.0]);
    assert_eq!(check, Ok(AreaOfUseCheck::Inside));
}

#[test]
fn millimetres_read_as_metres_are_refused() {
    // The Seattle coordinate above, scaled by 1000. The Lambert projection
    // inverts it to a valid position near the South Pole.
    let err = reprojector(2926)
        .check_area_of_use([1_270_000_000.0, 230_000_000.0, 50_000.0])
        .unwrap_err();
    let CrsError::OutsideAreaOfUse(message) = &err else {
        panic!("want OutsideAreaOfUse, got {err:?}");
    };
    assert!(message.contains("EPSG:2926"), "{message}");
    assert!(message.contains("area of use"), "{message}");
}

#[test]
fn web_mercator_beyond_its_latitude_band_is_a_warning() {
    // EPSG:3857 is valid between 85.06°S and 85.06°N. A northing of 1e9 m
    // inverts to the North Pole, 4.9 degrees outside the box. A latitude
    // cannot be 20 degrees outside a box that reaches 85°, so this case is a
    // warning and never a refusal.
    let check = reprojector(3857)
        .check_area_of_use([0.0, 1.0e9, 0.0])
        .unwrap();
    let AreaOfUseCheck::Outside(outside) = check else {
        panic!("want Outside, got {check:?}");
    };
    assert!(outside.lat_deg > 89.9, "{outside:?}");
    assert!((outside.excess_deg - 4.94).abs() < 0.01, "{outside:?}");
}

#[test]
fn feet_read_as_metres_are_a_warning() {
    // The Seattle coordinate in US survey feet, declared as the metre
    // variant of the same grid (EPSG:2855). It lands in Montana.
    let check = reprojector(2855)
        .check_area_of_use([1_270_000.0, 230_000.0, 50.0])
        .unwrap();
    let AreaOfUseCheck::Outside(outside) = check else {
        panic!("want Outside, got {check:?}");
    };
    assert!(
        outside.excess_deg > 1.0 && outside.excess_deg < 20.0,
        "{outside:?}"
    );
}

#[test]
fn national_grid_used_past_its_zone_is_a_warning() {
    // Norway uses ETRS89 / UTM zone 33N (nominal 12°E to 18°E) nationwide.
    // Kirkenes, at 30°E, is 12 degrees outside the box.
    let check = reprojector(25833)
        .check_area_of_use([1_079_000.0, 7_780_000.0, 20.0])
        .unwrap();
    let AreaOfUseCheck::Outside(outside) = check else {
        panic!("want Outside, got {check:?}");
    };
    assert_eq!(outside.epsg, 25833);
    assert!((outside.lon_deg - 30.0).abs() < 1.0, "{outside:?}");
    assert!(
        outside.excess_deg > 10.0 && outside.excess_deg < 14.0,
        "{outside:?}"
    );
    let message = outside.to_string();
    assert!(message.contains("EPSG:25833"), "{message}");
    assert!(message.contains("outside the area of use"), "{message}");
}

#[test]
fn data_a_short_way_past_the_box_edge_passes() {
    // 0.5 degrees west of the UTM zone 17N box.
    let check = reprojector(32617).check_area_of_use([211_000.0, 4_851_490.0, 0.0]);
    assert_eq!(check, Ok(AreaOfUseCheck::Inside));
}

#[test]
fn coordinate_valid_in_the_wrong_utm_hemisphere_is_not_detected() {
    // Documented limit. A northern northing read in WGS 84 / UTM zone 19S is
    // a valid position at 46°S, inside the box of the southern zone.
    let check = reprojector(32719).check_area_of_use([473_000.0, 4_851_490.0, 0.0]);
    assert_eq!(check, Ok(AreaOfUseCheck::Inside));
}

#[test]
fn height_does_not_affect_the_check() {
    // A height in the wrong unit must not turn an in-area point into a miss.
    let check = reprojector(32617).check_area_of_use([649_490.0, 4_851_490.0, 9.0e9]);
    assert_eq!(check, Ok(AreaOfUseCheck::Inside));
}

#[test]
fn ecef_source_has_no_area_to_check() {
    // tileforge-pc sends every local-frame input through EPSG:4978 with
    // small coordinates. The check must never refuse those.
    for xyz in [[0.0, 0.0, 0.0], [12.5, -3.0, 22.9], [6.4e9, 0.0, 0.0]] {
        let check = reprojector(4978).check_area_of_use(xyz);
        assert_eq!(check, Ok(AreaOfUseCheck::Unknown), "{xyz:?}");
    }
}

#[test]
fn geographic_source_is_checked_in_degrees() {
    // NAD83 geographic (EPSG:4269): North America.
    let check = reprojector(4269).check_area_of_use([-122.3, 47.6, 0.0]);
    assert_eq!(check, Ok(AreaOfUseCheck::Inside));
    // Paris is 60 degrees and more east of the NAD83 box.
    let err = reprojector(4269)
        .check_area_of_use([2.35, 48.86, 0.0])
        .unwrap_err();
    assert!(matches!(err, CrsError::OutsideAreaOfUse(_)), "{err:?}");
    // WGS 84 geographic covers the world.
    let check = reprojector(4326).check_area_of_use([2.35, 48.86, 0.0]);
    assert_eq!(check, Ok(AreaOfUseCheck::Inside));
}

#[test]
fn non_finite_coordinate_is_an_error() {
    let err = reprojector(32617)
        .check_area_of_use([f64::NAN, 4_851_490.0, 0.0])
        .unwrap_err();
    assert!(matches!(err, CrsError::Reproject(_)), "{err:?}");
}
