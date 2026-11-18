// ABOUTME: Answers questions about the proj4 catalogue definition of an EPSG code.
// ABOUTME: Tells whether a CRS is placed on WGS 84 without a datum shift.
//! Facts about catalogue definitions that the parser needs.

/// `true` when the crate places the CRS `epsg` on WGS 84 without a datum
/// shift, so that a zero-valued WKT `TOWGS84` clause states what the crate
/// does.
///
/// That holds for a definition that names the WGS 84 or NAD83 datum, that
/// carries a zero `+towgs84` or the null grid, or that names only a GRS80 or
/// WGS 84 ellipsoid or a sphere. It does not hold for a definition on any
/// other ellipsoid (its datum is offset from WGS 84), for a grid-based datum
/// such as NAD27, or for a code that is not in the catalogue.
pub(crate) fn has_zero_shift_to_wgs84(epsg: u16) -> bool {
    let Some(definition) = crs_definitions::from_code(epsg) else {
        return false;
    };
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
    // No datum: only the ellipsoid tells.
    if value("R").is_some() {
        return true;
    }
    match value("ellps") {
        Some(name) => matches!(name, "GRS80" | "WGS84" | "sphere"),
        None => match (number("a"), number("rf"), number("b")) {
            (Some(a), _, Some(b)) if a == b => true,
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
    fn zero_shift_holds_only_for_crs_placed_on_wgs84_as_they_are() {
        // WGS 84, NAD83 by name, GRS80 without a datum, Web Mercator, and a
        // sphere (EPSG:2163).
        for epsg in [4326_u16, 32617, 4269, 26917, 2992, 25832, 3857, 2163] {
            assert!(has_zero_shift_to_wgs84(epsg), "EPSG:{epsg}");
        }
        // Airy (OSGB36), IAG 1975 (Xian 1980), NAD27 grids, Bessel, not in
        // the catalogue.
        for epsg in [27700_u16, 2327, 26717, 28992, 60000, 0] {
            assert!(!has_zero_shift_to_wgs84(epsg), "EPSG:{epsg}");
        }
    }
}
