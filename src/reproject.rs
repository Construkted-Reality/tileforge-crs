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
        Ok(Self {
            source_epsg: source.epsg,
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
        let mut p = (xyz[0], xyz[1], xyz[2]);
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
