// ABOUTME: Answers questions about the proj4 catalogue definition of an EPSG code.
// ABOUTME: Tells whether a zero-valued WKT TOWGS84 clause is acceptable for a code.
//! Facts about catalogue definitions that the parser needs.

use crate::datum_shift::{DatumShiftRule, datum_shift_rule};

/// `true` when a zero-valued WKT `TOWGS84` clause is accepted for the CRS
/// `epsg`: the catalogue definition names the WGS 84 or NAD83 datum, carries
/// a zero `+towgs84` or the null grid, or names only the GRS80 or WGS 84
/// ellipsoid.
///
/// For these definitions `Reprojector` applies no datum shift, so the clause
/// changes nothing in the output. A GRS80 or WGS 84 based CRS that gets a
/// Helmert shift from the datum shift table (GGRS87, VN-2000, Israel 1993)
/// is not in this set: a zero clause contradicts the shift.
///
/// `false` for a definition on any other ellipsoid or on a sphere without
/// the null grid, for a grid-based datum such as NAD27, and for a code that
/// is not in the catalogue. A zero shift cannot be confirmed for those. The
/// null grid marks the Web Mercator codes (EPSG:3857, 3785, 6871), to which
/// PROJ applies no shift either.
pub(crate) fn accepts_zero_towgs84(epsg: u16) -> bool {
    let Some(definition) = crs_definitions::from_code(epsg) else {
        return false;
    };
    if datum_shift_rule(epsg) != DatumShiftRule::NotNeeded {
        return false;
    }
    let value = |key: &str| {
        definition
            .proj4
            .split_whitespace()
            .find_map(|t| t.strip_prefix('+')?.strip_prefix(key)?.strip_prefix('='))
    };
    let number = |key: &str| value(key).and_then(|v| v.parse::<f64>().ok());
    if let Some(grids) = value("nadgrids") {
        return grids == "@null";
    }
    if let Some(towgs84) = value("towgs84") {
        return towgs84
            .split(',')
            .all(|v| v.trim().parse::<f64>().is_ok_and(|shift| shift == 0.0));
    }
    if let Some(datum) = value("datum") {
        return matches!(datum, "WGS84" | "NAD83");
    }
    // No datum: only the ellipsoid tells. A sphere (`+R=`, or equal axes)
    // is not GRS80 or WGS 84.
    if value("R").is_some() {
        return false;
    }
    match value("ellps") {
        Some(name) => matches!(name, "GRS80" | "WGS84"),
        None => match (number("a"), number("rf"), number("b")) {
            (Some(a), Some(rf), _) => {
                let f = 1.0 / rf;
                is_wgs84_ellipsoid(a, f * (2.0 - f))
            }
            (Some(a), None, Some(b)) => is_wgs84_ellipsoid(a, 1.0 - (b * b) / (a * a)),
            _ => false,
        },
    }
}

/// The test that proj4rs uses to treat an ellipsoid as WGS 84: equal
/// semi-major axis, and an eccentricity squared within the distance between
/// GRS80 and WGS 84.
fn is_wgs84_ellipsoid(a: f64, es: f64) -> bool {
    a == 6_378_137.0 && (es - 0.006_694_379_990_141_316_5).abs() < 0.000_000_000_050
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_towgs84_is_accepted_only_on_grs80_or_wgs84() {
        // WGS 84 and NAD83 by name, GRS80 without a datum, Web Mercator
        // (null grid).
        for epsg in [4326_u16, 32617, 4269, 26917, 2992, 25832, 3857] {
            assert!(accepts_zero_towgs84(epsg), "EPSG:{epsg}");
        }
        // Airy (OSGB36), IAG 1975 (Xian 1980), NAD27 grids, Bessel, spheres
        // (EPSG:2163, 3408, 4047), GRS80 or WGS 84 based CRSs that get a
        // Helmert shift (GGRS87, Israel 1993, VN-2000), not in the catalogue.
        for epsg in [
            27700_u16, 2327, 26717, 28992, 2163, 3408, 4047, 2100, 2039, 3405, 60000, 0,
        ] {
            assert!(!accepts_zero_towgs84(epsg), "EPSG:{epsg}");
        }
    }
}
