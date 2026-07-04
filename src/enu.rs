//! `EnuFrame` — a local East-North-Up tangent frame at an ECEF origin.
//!
//! tileforge-mesh's georeferencing (ADR-041) reprojects every vertex to ECEF
//! (via [`crate::Reprojector`]), then into a **local ENU metric frame** centred
//! at the dataset origin, so the whole metric pipeline runs unchanged and
//! tile-local f32 stays sub-mm. The single root `local→ECEF` transform written
//! to `tileset.json` is exactly this frame's [`EnuFrame::enu_to_ecef_matrix`].
//!
//! Axis mapping is **East→X, North→Y, Up→Z** (ADR-041 D3/R1): the matrix's
//! first three columns are the East/North/Up basis vectors and the fourth is
//! the ECEF origin, so `M · [x,y,z,1]ᵀ_localENU = ECEF`.

use proj4rs::Proj;
use proj4rs::transform::transform;

use crate::error::CrsError;

const EPSG_ECEF: u16 = 4978;
const EPSG_WGS84: u16 = 4326;

/// Minimum plausible radius (metres) for a georeferenced ECEF surface
/// anchor. Earth's polar radius is ~6.357e6 m; the deepest ocean trench
/// is ~11 km below the ellipsoid, so any genuine surface/near-surface
/// point has `|ecef| ≳ 6.345e6 m`. A radius below this floor means the
/// input is degenerate — the geocenter `[0,0,0]`, a deep-interior point,
/// or an ENU-local value fed in as ECEF by mistake — not a real anchor.
/// proj4rs's geocentric inverse does NOT reject such points (it maps
/// `[0,0,0]` to the north pole), so we guard here (F2).
const MIN_ECEF_RADIUS_M: f64 = 6.2e6;

#[inline]
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Geodetic longitude/latitude (radians) of an ECEF point, via
/// proj4rs EPSG:4978 → EPSG:4326. Returns `(lon_rad, lat_rad)` (proj4rs
/// emits geographic coordinates as `(lon, lat)` in radians).
pub fn ecef_to_geodetic_lonlat(ecef: [f64; 3]) -> Result<(f64, f64), CrsError> {
    // Reject degenerate / non-finite origins (F2). proj4rs's geocentric
    // inverse silently maps the geocenter and deep-interior points to a
    // "valid" pole/near-pole answer and passes NaN straight through,
    // which would anchor a whole tileset at the wrong place with no error
    // anywhere. A real georeferenced ECEF point sits on/near the ellipsoid
    // surface; guard both non-finite input and an implausibly small radius.
    if !ecef.iter().all(|c| c.is_finite()) {
        return Err(CrsError::reproject(format!(
            "non-finite ECEF origin ({}, {}, {})",
            ecef[0], ecef[1], ecef[2]
        )));
    }
    let radius = (ecef[0] * ecef[0] + ecef[1] * ecef[1] + ecef[2] * ecef[2]).sqrt();
    if radius < MIN_ECEF_RADIUS_M {
        return Err(CrsError::reproject(format!(
            "degenerate ECEF origin ({}, {}, {}): radius {radius} m is below the \
             {MIN_ECEF_RADIUS_M} m minimum for a georeferenced surface anchor",
            ecef[0], ecef[1], ecef[2]
        )));
    }
    let from = Proj::from_epsg_code(EPSG_ECEF)
        .map_err(|e| CrsError::unknown_epsg(format!("EPSG:{EPSG_ECEF} missing: {e:?}")))?;
    let to = Proj::from_epsg_code(EPSG_WGS84)
        .map_err(|e| CrsError::unknown_epsg(format!("EPSG:{EPSG_WGS84} missing: {e:?}")))?;
    let mut p = (ecef[0], ecef[1], ecef[2]);
    transform(&from, &to, &mut p).map_err(|e| {
        CrsError::reproject(format!(
            "ECEF→WGS84 for ({}, {}, {}) failed: {e:?}",
            ecef[0], ecef[1], ecef[2]
        ))
    })?;
    Ok((p.0, p.1))
}

/// A local East-North-Up tangent frame anchored at an ECEF origin.
///
/// Build with [`EnuFrame::from_ecef_origin`] (origin = the dataset centroid
/// reprojected to ECEF). [`EnuFrame::ecef_to_enu`] maps a reprojected vertex
/// into local metres; [`EnuFrame::enu_to_ecef_matrix`] is the root transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnuFrame {
    origin_ecef: [f64; 3],
    lon_rad: f64,
    lat_rad: f64,
    east: [f64; 3],
    north: [f64; 3],
    up: [f64; 3],
}

impl EnuFrame {
    /// Build the ENU frame whose local origin `(0,0,0)` is `origin_ecef`
    /// (ECEF metres). Orients the tangent plane from the geodetic lat/lon
    /// at that origin (computed via EPSG:4978 → EPSG:4326).
    pub fn from_ecef_origin(origin_ecef: [f64; 3]) -> Result<Self, CrsError> {
        let (lon, lat) = ecef_to_geodetic_lonlat(origin_ecef)?;
        let (slon, clon) = lon.sin_cos();
        let (slat, clat) = lat.sin_cos();
        // Standard ENU basis at (lon, lat), expressed in ECEF axes.
        let east = [-slon, clon, 0.0];
        let north = [-slat * clon, -slat * slon, clat];
        let up = [clat * clon, clat * slon, slat];
        Ok(Self {
            origin_ecef,
            lon_rad: lon,
            lat_rad: lat,
            east,
            north,
            up,
        })
    }

    /// Map an ECEF point into local ENU metres relative to this frame's
    /// origin. `(X,Y,Z) = (East, North, Up)`.
    pub fn ecef_to_enu(&self, p: [f64; 3]) -> [f64; 3] {
        let d = [
            p[0] - self.origin_ecef[0],
            p[1] - self.origin_ecef[1],
            p[2] - self.origin_ecef[2],
        ];
        [dot(self.east, d), dot(self.north, d), dot(self.up, d)]
    }

    /// Map a local ENU point (metres, `(X,Y,Z) = (East,North,Up)`) back to
    /// ECEF metres. The exact inverse of [`EnuFrame::ecef_to_enu`]: applies
    /// the ENU→ECEF rotation (the East/North/Up basis as columns) and adds
    /// the origin. Equivalent to multiplying by [`EnuFrame::enu_to_ecef_matrix`]
    /// but without materialising the matrix — used by consumers that
    /// partition in the ENU frame and bake coordinates back to absolute ECEF
    /// per point (rather than emitting a tileset root transform).
    pub fn enu_to_ecef(&self, enu: [f64; 3]) -> [f64; 3] {
        [
            self.east[0] * enu[0]
                + self.north[0] * enu[1]
                + self.up[0] * enu[2]
                + self.origin_ecef[0],
            self.east[1] * enu[0]
                + self.north[1] * enu[1]
                + self.up[1] * enu[2]
                + self.origin_ecef[1],
            self.east[2] * enu[0]
                + self.north[2] * enu[1]
                + self.up[2] * enu[2]
                + self.origin_ecef[2],
        ]
    }

    /// The 4×4 **column-major** `local-ENU → ECEF` matrix — the 3D Tiles
    /// root `transform`. Columns: East, North, Up (rotation), then the ECEF
    /// origin (translation). `M · [x,y,z,1]ᵀ = ECEF`.
    pub fn enu_to_ecef_matrix(&self) -> [f64; 16] {
        [
            self.east[0],
            self.east[1],
            self.east[2],
            0.0, // col 0: East → local X
            self.north[0],
            self.north[1],
            self.north[2],
            0.0, // col 1: North → local Y
            self.up[0],
            self.up[1],
            self.up[2],
            0.0, // col 2: Up → local Z
            self.origin_ecef[0],
            self.origin_ecef[1],
            self.origin_ecef[2],
            1.0, // col 3: origin
        ]
    }

    pub fn origin_ecef(&self) -> [f64; 3] {
        self.origin_ecef
    }

    /// Geodetic `(lon_rad, lat_rad)` at the frame origin (provenance).
    pub fn geodetic_lon_lat_rad(&self) -> (f64, f64) {
        (self.lon_rad, self.lat_rad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A point near Žilina, Slovakia (the ADR-041 driver model centroid):
    // lon 18.2906°, lat 49.0297°, h ~178 m. Its ECEF, from proj4rs.
    fn sample_ecef() -> [f64; 3] {
        ecef_of(18.2906, 49.0297, 178.0)
    }

    /// lon/lat (degrees) + height → ECEF via proj4rs (4326→4978).
    fn ecef_of(lon_deg: f64, lat_deg: f64, h: f64) -> [f64; 3] {
        let from = Proj::from_epsg_code(EPSG_WGS84).unwrap();
        let to = Proj::from_epsg_code(EPSG_ECEF).unwrap();
        let mut p = (lon_deg.to_radians(), lat_deg.to_radians(), h);
        transform(&from, &to, &mut p).unwrap();
        [p.0, p.1, p.2]
    }

    #[test]
    fn degenerate_or_non_finite_origin_is_rejected() {
        // F2: proj4rs's geocentric inverse maps [0,0,0] to the north pole
        // and passes NaN through as Ok((NaN, NaN)). Both the low-level
        // ecef_to_geodetic_lonlat and EnuFrame::from_ecef_origin must
        // reject degenerate + non-finite origins instead.
        let bad_origins = [
            [0.0, 0.0, 0.0],                              // geocenter → pole
            [1.0, 1.0, 1.0],                              // deep interior
            [f64::NAN, 0.0, 0.0],                         // non-finite
            [0.0, f64::INFINITY, 0.0],                    // non-finite
            [1.0e6, 1.0e6, 1.0e6],                        // radius ~1.7e6 m
            [f64::NEG_INFINITY, f64::NAN, f64::INFINITY], // fully non-finite
        ];
        for o in bad_origins {
            assert!(
                ecef_to_geodetic_lonlat(o).is_err(),
                "ecef_to_geodetic_lonlat({o:?}) must be rejected"
            );
            assert!(
                EnuFrame::from_ecef_origin(o).is_err(),
                "from_ecef_origin({o:?}) must be rejected"
            );
        }
        // A genuine surface anchor still succeeds.
        assert!(ecef_to_geodetic_lonlat(sample_ecef()).is_ok());
    }

    #[test]
    fn ecef_to_geodetic_lonlat_recovers_known_lon_lat() {
        // MT4: direct value assert. ecef_to_geodetic_lonlat is the exact
        // inverse of the 4326→4978 forward transform, so a Seattle point
        // must round-trip to its input lon/lat.
        let (lon_deg, lat_deg, h) = (-122.3321_f64, 47.6062_f64, 56.0);
        let (lon, lat) = ecef_to_geodetic_lonlat(ecef_of(lon_deg, lat_deg, h)).unwrap();
        assert!(
            (lon.to_degrees() - lon_deg).abs() < 1e-9,
            "lon: got {}",
            lon.to_degrees()
        );
        assert!(
            (lat.to_degrees() - lat_deg).abs() < 1e-9,
            "lat: got {}",
            lat.to_degrees()
        );
    }

    #[test]
    fn basis_is_orthonormal_at_extreme_anchors() {
        // MT4: the four ENU tests otherwise use a single mid-northern
        // point. Sweep poles, both hemispheres, and the antimeridian.
        let anchors = [
            (0.0, 89.9, 0.0),      // near north pole
            (10.0, -89.9, 0.0),    // near south pole
            (-122.4, 47.6, 500.0), // northern + western
            (-70.6, -33.4, 500.0), // southern + western (Santiago)
            (180.0, 0.0, 0.0),     // antimeridian, equator
            (179.9, -45.0, 0.0),   // antimeridian, southern
        ];
        for (lon, lat, h) in anchors {
            let f = EnuFrame::from_ecef_origin(ecef_of(lon, lat, h))
                .unwrap_or_else(|e| panic!("frame at ({lon},{lat},{h}): {e}"));
            for v in [f.east, f.north, f.up] {
                assert!(
                    (dot(v, v) - 1.0).abs() < 1e-9,
                    "unit length at ({lon},{lat})"
                );
            }
            assert!(dot(f.east, f.north).abs() < 1e-9, "E·N at ({lon},{lat})");
            assert!(dot(f.east, f.up).abs() < 1e-9, "E·U at ({lon},{lat})");
            assert!(dot(f.north, f.up).abs() < 1e-9, "N·U at ({lon},{lat})");
        }
    }

    #[test]
    fn basis_is_orthonormal() {
        let f = EnuFrame::from_ecef_origin(sample_ecef()).unwrap();
        for v in [f.east, f.north, f.up] {
            assert!((dot(v, v) - 1.0).abs() < 1e-9, "unit length");
        }
        assert!(dot(f.east, f.north).abs() < 1e-9);
        assert!(dot(f.east, f.up).abs() < 1e-9);
        assert!(dot(f.north, f.up).abs() < 1e-9);
    }

    #[test]
    fn origin_maps_to_local_zero() {
        let o = sample_ecef();
        let f = EnuFrame::from_ecef_origin(o).unwrap();
        let enu = f.ecef_to_enu(o);
        for c in enu {
            assert!(c.abs() < 1e-6, "origin → ~0 local, got {enu:?}");
        }
    }

    #[test]
    fn enu_to_ecef_inverts_ecef_to_enu() {
        // enu_to_ecef must be the exact inverse of ecef_to_enu, and must
        // agree with the column-major matrix form.
        let f = EnuFrame::from_ecef_origin(sample_ecef()).unwrap();
        let local = [123.456_f64, -78.9, 12.34];
        let ecef = f.enu_to_ecef(local);
        let back = f.ecef_to_enu(ecef);
        for (a, b) in back.iter().zip(local.iter()) {
            assert!((a - b).abs() < 1e-6, "round-trip: {back:?} vs {local:?}");
        }
        let m = f.enu_to_ecef_matrix();
        let via_matrix = [
            m[0] * local[0] + m[4] * local[1] + m[8] * local[2] + m[12],
            m[1] * local[0] + m[5] * local[1] + m[9] * local[2] + m[13],
            m[2] * local[0] + m[6] * local[1] + m[10] * local[2] + m[14],
        ];
        for (a, b) in ecef.iter().zip(via_matrix.iter()) {
            assert!(
                (a - b).abs() < 1e-9,
                "enu_to_ecef vs matrix: {ecef:?} vs {via_matrix:?}"
            );
        }
    }

    #[test]
    fn matrix_round_trips_a_local_point() {
        // A vertex 100 m East, 50 m North, 10 m Up of the origin should,
        // when pushed through the enu→ecef matrix and back through
        // ecef_to_enu, recover (100, 50, 10).
        let f = EnuFrame::from_ecef_origin(sample_ecef()).unwrap();
        let m = f.enu_to_ecef_matrix();
        let local = [100.0_f64, 50.0, 10.0];
        // column-major M · [local,1]
        let ecef = [
            m[0] * local[0] + m[4] * local[1] + m[8] * local[2] + m[12],
            m[1] * local[0] + m[5] * local[1] + m[9] * local[2] + m[13],
            m[2] * local[0] + m[6] * local[1] + m[10] * local[2] + m[14],
        ];
        let back = f.ecef_to_enu(ecef);
        for (a, b) in back.iter().zip(local.iter()) {
            assert!((a - b).abs() < 1e-6, "round-trip: {back:?} vs {local:?}");
        }
    }
}
