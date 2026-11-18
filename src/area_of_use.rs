// ABOUTME: Checks a source coordinate against the EPSG area of use of its CRS.
// ABOUTME: Reports a position outside the area as a warning, and a gross miss as an error.
//! Area-of-use plausibility check.
//!
//! A projection is a mathematical function. It returns a position for almost
//! any input, including coordinates in the wrong unit (millimetres read as
//! metres) or coordinates that belong to another CRS. The result is then a
//! valid position in the wrong part of the world.
//!
//! EPSG records, for each CRS, the geographic box in which the CRS is meant to
//! be used. This module compares one representative source coordinate (the
//! dataset centre) with that box. The check cannot detect a wrong CRS whose
//! coordinates happen to fall inside the box.

use std::fmt;

use crate::area_of_use_table::AREAS_OF_USE;

/// A position this many degrees outside the box still counts as inside.
/// EPSG boxes are rounded to 0.01 degree and follow coastlines and borders
/// loosely; data a short way over the edge is ordinary.
pub(crate) const TOLERANCE_DEG: f64 = 1.0;

/// A position more than this many degrees outside the box is refused.
/// The margin is wide on purpose. National data sets are in ordinary use far
/// outside the nominal box of their CRS: Norway uses ETRS89 / UTM zone 33N
/// (nominal 12°E to 18°E) from 4°E to 31°E, which is 13 degrees outside.
pub(crate) const REFUSAL_MARGIN_DEG: f64 = 20.0;

/// The EPSG area of use of a CRS: a geographic bounding box in degrees.
///
/// `west_deg > east_deg` means that the box crosses the antimeridian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaOfUse {
    pub west_deg: f64,
    pub south_deg: f64,
    pub east_deg: f64,
    pub north_deg: f64,
}

impl AreaOfUse {
    /// The area of use that the catalogue records for `epsg`, if any.
    pub(crate) fn for_epsg(epsg: u16) -> Option<Self> {
        let i = AREAS_OF_USE
            .binary_search_by_key(&epsg, |&(code, _)| code)
            .ok()?;
        let [west, south, east, north] = AREAS_OF_USE[i].1;
        Some(Self {
            west_deg: f64::from(west) / 100.0,
            south_deg: f64::from(south) / 100.0,
            east_deg: f64::from(east) / 100.0,
            north_deg: f64::from(north) / 100.0,
        })
    }

    /// How far `(lon_deg, lat_deg)` lies outside the box, in degrees: the
    /// larger of the latitude excess and the longitude excess. `0.0` inside.
    pub(crate) fn excess_deg(&self, lon_deg: f64, lat_deg: f64) -> f64 {
        let lat_excess = (self.south_deg - lat_deg)
            .max(lat_deg - self.north_deg)
            .max(0.0);
        // Longitude is cyclic: measure eastward from the west bound.
        let width = (self.east_deg - self.west_deg).rem_euclid(360.0);
        let width = if width == 0.0 && self.east_deg != self.west_deg {
            360.0
        } else {
            width
        };
        let offset = (lon_deg - self.west_deg).rem_euclid(360.0);
        let lon_excess = if offset <= width {
            0.0
        } else {
            // Past the east bound, or before the west bound, whichever is nearer.
            (offset - width).min(360.0 - offset)
        };
        lat_excess.max(lon_excess)
    }
}

impl fmt::Display for AreaOfUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "longitude {} to {}, latitude {} to {}",
            self.west_deg, self.east_deg, self.south_deg, self.north_deg
        )
    }
}

/// A position that lies outside the area of use of its CRS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutsideAreaOfUse {
    /// EPSG code of the source CRS.
    pub epsg: u16,
    /// WGS 84 longitude of the checked position, degrees east.
    pub lon_deg: f64,
    /// WGS 84 latitude of the checked position, degrees north.
    pub lat_deg: f64,
    /// The EPSG area of use of the source CRS.
    pub area: AreaOfUse,
    /// Distance outside the box in degrees: the larger of the latitude
    /// excess and the longitude excess.
    pub excess_deg: f64,
}

impl fmt::Display for OutsideAreaOfUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the data lies at longitude {:.4}, latitude {:.4}, which is {:.1} degrees outside \
             the area of use of EPSG:{} ({}); check the CRS and the coordinate unit of the input",
            self.lon_deg, self.lat_deg, self.excess_deg, self.epsg, self.area
        )
    }
}

/// Outcome of [`Reprojector::check_area_of_use`](crate::Reprojector::check_area_of_use)
/// for a position that is not refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AreaOfUseCheck {
    /// The position is inside the area of use, or within 1 degree of it.
    Inside,
    /// The position is 1 to 20 degrees outside the area of use. The data can
    /// be valid (a national grid used past its nominal zone), so this is a
    /// warning for the consumer to pass on, not an error.
    Outside(OutsideAreaOfUse),
    /// The catalogue records no area of use for the source CRS, or the source
    /// is EPSG:4978 (ECEF), which has no horizontal position to check.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_boxes_are_in_range() {
        assert!(AREAS_OF_USE.windows(2).all(|w| w[0].0 < w[1].0));
        for &(code, [west, south, east, north]) in &AREAS_OF_USE {
            assert!((-18000..=18000).contains(&west), "EPSG:{code}");
            assert!((-18000..=18000).contains(&east), "EPSG:{code}");
            assert!((-9000..=9000).contains(&south), "EPSG:{code}");
            assert!(south <= north && north <= 9000, "EPSG:{code}");
        }
    }

    #[test]
    fn area_of_use_matches_the_epsg_record() {
        // EPSG:2926, NAD83(HARN) / Washington North (ftUS).
        let area = AreaOfUse::for_epsg(2926).unwrap();
        assert_eq!(
            area,
            AreaOfUse {
                west_deg: -124.79,
                south_deg: 47.08,
                east_deg: -117.02,
                north_deg: 49.05
            }
        );
        assert_eq!(AreaOfUse::for_epsg(60000), None);
    }

    #[test]
    fn excess_is_zero_inside_and_grows_outside() {
        let area = AreaOfUse {
            west_deg: 12.0,
            south_deg: 0.0,
            east_deg: 18.0,
            north_deg: 84.0,
        };
        assert_eq!(area.excess_deg(15.0, 60.0), 0.0);
        assert_eq!(area.excess_deg(12.0, 0.0), 0.0);
        assert!((area.excess_deg(31.0, 70.0) - 13.0).abs() < 1e-9);
        assert!((area.excess_deg(4.0, 60.0) - 8.0).abs() < 1e-9);
        assert!((area.excess_deg(15.0, -46.0) - 46.0).abs() < 1e-9);
        // The far side of the globe is 177 degrees from either bound.
        assert!((area.excess_deg(-165.0, 60.0) - 177.0).abs() < 1e-9);
    }

    #[test]
    fn excess_handles_a_box_across_the_antimeridian() {
        // Like EPSG:32601 neighbours and Pacific grids: 174°E to 174°W.
        let area = AreaOfUse {
            west_deg: 174.0,
            south_deg: -50.0,
            east_deg: -174.0,
            north_deg: -30.0,
        };
        assert_eq!(area.excess_deg(179.5, -40.0), 0.0);
        assert_eq!(area.excess_deg(-179.5, -40.0), 0.0);
        assert!((area.excess_deg(170.0, -40.0) - 4.0).abs() < 1e-9);
        assert!((area.excess_deg(-170.0, -40.0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn world_box_has_no_longitude_excess() {
        let area = AreaOfUse {
            west_deg: -180.0,
            south_deg: -90.0,
            east_deg: 180.0,
            north_deg: 90.0,
        };
        for lon in [-180.0, -90.0, 0.0, 179.99, 180.0] {
            assert_eq!(area.excess_deg(lon, 45.0), 0.0, "lon {lon}");
        }
    }
}
