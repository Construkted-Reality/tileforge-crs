//! `Reprojector` — wraps proj4rs to reproject from any catalogued source
//! EPSG into ECEF (EPSG:4978).
//!
//! The reprojection target is fixed at EPSG:4978 (geocentric metres) per
//! the 3D Tiles 1.1 box-bounding-volume contract. Source EPSGs are
//! resolved through proj4rs's `crs-definitions` feature, which carries
//! the proj4 EPSG catalogue.

use proj4rs::Proj;
use proj4rs::transform::transform;

use crate::error::CrsError;
use crate::source_crs::SourceCrs;

/// EPSG code for ECEF metres. Fixed reprojection target.
const EPSG_ECEF: u16 = 4978;

/// Reprojector from a fixed source EPSG to ECEF (EPSG:4978).
///
/// Construction validates the source EPSG against the `crs-definitions`
/// catalogue; subsequent `to_ecef` calls are pure numerical work.
pub struct Reprojector {
    source_epsg: u16,
    /// Whether the source CRS is geographic (lat/long). proj4rs expects
    /// **radians** for geographic input, so `to_ecef` converts degrees→
    /// radians for the horizontal pair when this is true (ADR-041 C1).
    /// False for projected/geocentric sources → no conversion (PC's UTM/
    /// ECEF path is byte-identical).
    source_is_latlong: bool,
    source: Proj,
    target: Proj,
}

impl std::fmt::Debug for Reprojector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reprojector")
            .field("source_epsg", &self.source_epsg)
            .field("target_epsg", &EPSG_ECEF)
            .finish()
    }
}

impl Reprojector {
    /// Build a reprojector for `source → EPSG:4978`. Fails with
    /// [`CrsError::UnknownEpsg`] if the source EPSG isn't in the proj4
    /// catalogue.
    pub fn new(source: SourceCrs) -> Result<Self, CrsError> {
        let source_proj = Proj::from_epsg_code(source.epsg).map_err(|e| {
            CrsError::unknown_epsg(format!(
                "EPSG:{} not in crs-definitions catalogue: {e:?}",
                source.epsg
            ))
        })?;
        let target_proj = Proj::from_epsg_code(EPSG_ECEF).map_err(|e| {
            // This should be infallible — EPSG:4978 is in every build of
            // crs-definitions — but treat the error path as a real error
            // rather than panicking, to keep the tracing trail intact.
            CrsError::unknown_epsg(format!(
                "EPSG:{EPSG_ECEF} (ECEF target) missing from catalogue: {e:?}"
            ))
        })?;
        let source_is_latlong = source_proj.is_latlong();
        Ok(Self {
            source_epsg: source.epsg,
            source_is_latlong,
            source: source_proj,
            target: target_proj,
        })
    }

    /// Reproject `[x, y, z]` from the source CRS to ECEF metres.
    ///
    /// Identity (source EPSG == 4978) short-circuits without calling
    /// proj4rs, which keeps the bit-equal semantics the integration
    /// tests rely on.
    pub fn to_ecef(&self, xyz: [f64; 3]) -> Result<[f64; 3], CrsError> {
        if self.is_identity() {
            return Ok(xyz);
        }
        // proj4rs wants RADIANS for geographic (lat/long) sources; degrees
        // would silently produce garbage ECEF (ADR-041 C1). Height is metres.
        let mut p = if self.source_is_latlong {
            (xyz[0].to_radians(), xyz[1].to_radians(), xyz[2])
        } else {
            (xyz[0], xyz[1], xyz[2])
        };
        transform(&self.source, &self.target, &mut p).map_err(|e| {
            CrsError::reproject(format!(
                "reproject EPSG:{} → EPSG:{EPSG_ECEF} for ({}, {}, {}) failed: {e:?}",
                self.source_epsg, xyz[0], xyz[1], xyz[2]
            ))
        })?;
        Ok([p.0, p.1, p.2])
    }

    /// `true` iff the source EPSG is 4978 — i.e. reprojection is the
    /// identity.
    pub fn is_identity(&self) -> bool {
        self.source_epsg == EPSG_ECEF
    }
}

/// GeoTIFF reserved sentinel codes: `0` ("undefined") and `32767`
/// ("user-defined"). Per the GeoTIFF spec these are **not** EPSG codes —
/// a GeoKey, WKT `AUTHORITY`, or sidecar that carries one of them is
/// declaring "I have no standard CRS", not naming a projection. Consumers
/// should treat a sentinel as *absence* (fall through their CRS cascade
/// to local-frame), never feed it to [`Reprojector::new`] / the proj4
/// catalogue. The private/user range `32768..=65535` is left out
/// deliberately: those are implementation-defined and a consumer that
/// genuinely uses one should hit the normal "unresolvable EPSG" error
/// path rather than be silently localised.
pub const fn is_geotiff_sentinel(epsg: u16) -> bool {
    matches!(epsg, 0 | 32767)
}

/// `true` iff `epsg` resolves in the proj4 `crs-definitions` catalogue —
/// i.e. [`Reprojector::new`] would succeed for it. A cheap pre-flight
/// for `--crs` validation and friendly error messages: it builds (and
/// drops) only the source `Proj`, not the full reprojector, and never
/// touches the fixed ECEF target. Returns `false` for sentinels and for
/// any code absent from the catalogue.
pub fn is_supported_epsg(epsg: u16) -> bool {
    Proj::from_epsg_code(epsg).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecef_to_geodetic_lonlat;

    #[test]
    fn geotiff_sentinels_are_flagged() {
        assert!(is_geotiff_sentinel(0), "0 = undefined");
        assert!(is_geotiff_sentinel(32767), "32767 = user-defined");
    }

    #[test]
    fn real_epsg_codes_are_not_sentinels() {
        for code in [4326, 4978, 32617, 32719, 26917] {
            assert!(!is_geotiff_sentinel(code), "{code} is a real EPSG");
        }
        // Private/user range is intentionally NOT a sentinel.
        assert!(!is_geotiff_sentinel(32768));
    }

    #[test]
    fn is_supported_epsg_matches_catalogue() {
        // Catalogued codes the reprojector can build.
        assert!(is_supported_epsg(4978), "ECEF target is always present");
        assert!(is_supported_epsg(4326));
        assert!(is_supported_epsg(32617));
        // Sentinels and out-of-catalogue codes are unsupported.
        assert!(!is_supported_epsg(32767), "user-defined sentinel");
        assert!(!is_supported_epsg(0), "undefined sentinel");
        // A code that is not in the proj4 catalogue.
        assert!(!is_supported_epsg(60000));
    }

    #[test]
    fn geographic_source_round_trips_degrees() {
        // WGS84 lat/long (EPSG:4326) in DEGREES → ECEF → back to geodetic
        // must recover the input degrees. Confirms the degrees→radians
        // handling for geographic sources (ADR-041 C1): degrees-in without
        // the conversion would produce nonsense-scale ECEF and fail the
        // magnitude check below.
        let rp = Reprojector::new(SourceCrs::new(4326)).expect("4326 in catalogue");
        assert!(
            rp.source_is_latlong,
            "EPSG:4326 must be detected geographic"
        );
        let (lon_deg, lat_deg, h) = (18.2912_f64, 49.0305_f64, 175.0_f64);
        let ecef = rp.to_ecef([lon_deg, lat_deg, h]).expect("reproject");
        let mag = (ecef[0].powi(2) + ecef[1].powi(2) + ecef[2].powi(2)).sqrt();
        assert!(
            (mag - 6.369e6).abs() < 5.0e4,
            "ECEF magnitude must be ~Earth radius (degrees treated as radians), got {mag}"
        );
        let (lon_r, lat_r) = ecef_to_geodetic_lonlat(ecef).expect("ecef→geodetic");
        assert!(
            (lon_r.to_degrees() - lon_deg).abs() < 1e-6,
            "lon round-trip"
        );
        assert!(
            (lat_r.to_degrees() - lat_deg).abs() < 1e-6,
            "lat round-trip"
        );
    }

    #[test]
    fn projected_source_not_flagged_latlong() {
        // UTM zone 33N (EPSG:32633) is projected metres → no radians
        // conversion (PC's path stays byte-identical).
        let rp = Reprojector::new(SourceCrs::new(32633)).expect("32633 in catalogue");
        assert!(!rp.source_is_latlong, "UTM must not be flagged geographic");
    }
}
