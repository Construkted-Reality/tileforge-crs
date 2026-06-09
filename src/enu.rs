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

#[inline]
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Geodetic longitude/latitude (radians) of an ECEF point, via
/// proj4rs EPSG:4978 → EPSG:4326. Returns `(lon_rad, lat_rad)` (proj4rs
/// emits geographic coordinates as `(lon, lat)` in radians).
pub fn ecef_to_geodetic_lonlat(ecef: [f64; 3]) -> Result<(f64, f64), CrsError> {
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
        // lon/lat → ECEF via proj4rs (4326→4978), radians in.
        let from = Proj::from_epsg_code(EPSG_WGS84).unwrap();
        let to = Proj::from_epsg_code(EPSG_ECEF).unwrap();
        let mut p = (18.2906_f64.to_radians(), 49.0297_f64.to_radians(), 178.0);
        transform(&from, &to, &mut p).unwrap();
        [p.0, p.1, p.2]
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
