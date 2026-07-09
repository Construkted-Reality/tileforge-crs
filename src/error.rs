//! `CrsError` — the crate-local, consumer-agnostic error type.
//!
//! `tileforge-crs` is shared by `tileforge-pc` and `tileforge-mesh`,
//! which carry their own schema-versioned error envelopes. The core
//! therefore does NOT depend on either project's error crate; it
//! reports a small `thiserror` enum that each consumer maps onto its own
//! schema (PC's `tileforge_pc_errors::ErrorKind::CrsError(String)`,
//! mesh's schema-v1 errors) at the call boundary.
//!
//! The variants cover exactly the failure modes the core produces:
//!
//! - [`CrsError::UnknownEpsg`] — `Reprojector::new` could not resolve a
//!   source EPSG (or the fixed ECEF target) against the proj4
//!   `crs-definitions` catalogue.
//! - [`CrsError::Reproject`] — `Reprojector::to_ecef` failed to transform
//!   a point.
//! - [`CrsError::Parse`] — a CRS string / WKT body failed to parse into
//!   an EPSG code (`parse_crs_string`, `extract_epsg_from_wkt`).
//! - [`CrsError::SentinelCode`] — a CRS string parsed to a GeoTIFF reserved
//!   sentinel (`0` / `32767`), which is *not* a resolvable CRS code but a
//!   declaration of "no standard CRS" (`parse_crs_string`).
//! - [`CrsError::Io`] — reading a sidecar `.prj` / `.qpj` file failed
//!   (`detect_crs_from_sidecar`).

/// Consumer-agnostic CRS error. Each `tileforge-*` consumer maps this
/// onto its own error schema at the boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CrsError {
    /// An EPSG code was not present in the proj4 `crs-definitions`
    /// catalogue (source EPSG, or the fixed EPSG:4978 ECEF target).
    #[error("{0}")]
    UnknownEpsg(String),

    /// A point reprojection (source → ECEF) failed inside proj4rs.
    #[error("{0}")]
    Reproject(String),

    /// A CRS string or WKT body failed to parse into an EPSG code.
    #[error("{0}")]
    Parse(String),

    /// A CRS string parsed to a GeoTIFF reserved sentinel code (`0` =
    /// "undefined", `32767` = "user-defined"). Per the GeoTIFF spec these
    /// are **not** EPSG codes — they declare "I have no standard CRS", not a
    /// projection — so the parser rejects them rather than handing back a
    /// non-resolvable code. Consumers that treat a sentinel as *absence*
    /// (fall through to a local frame) should match this variant and/or use
    /// [`is_geotiff_sentinel`](crate::is_geotiff_sentinel). Carries the
    /// sentinel value that was seen.
    #[error(
        "EPSG:{0} is a GeoTIFF reserved sentinel (0 = undefined, 32767 = \
         user-defined), not a resolvable coordinate reference system; treat \
         it as 'no CRS' (absence), do not pass it to the proj4 catalogue"
    )]
    SentinelCode(u16),

    /// An I/O error reading a sidecar `.prj` / `.qpj` file. Carries the
    /// path and the underlying message so consumers can re-tag it as
    /// their own I/O error variant.
    #[error("{path}: {detail}")]
    Io { path: String, detail: String },
}

impl CrsError {
    /// Construct an [`CrsError::UnknownEpsg`].
    pub fn unknown_epsg(msg: impl Into<String>) -> Self {
        Self::UnknownEpsg(msg.into())
    }

    /// Construct a [`CrsError::Reproject`].
    pub fn reproject(msg: impl Into<String>) -> Self {
        Self::Reproject(msg.into())
    }

    /// Construct a [`CrsError::Parse`].
    pub fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }

    /// Construct a [`CrsError::SentinelCode`] from the sentinel value seen.
    pub fn sentinel_code(epsg: u16) -> Self {
        Self::SentinelCode(epsg)
    }

    /// Construct a [`CrsError::Io`] from a path and an underlying error.
    pub fn io(path: impl AsRef<std::path::Path>, source: impl std::fmt::Display) -> Self {
        Self::Io {
            path: path.as_ref().display().to_string(),
            detail: source.to_string(),
        }
    }
}
