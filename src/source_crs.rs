//! Canonical source-CRS handle.
//!
//! A thin newtype around a `u16` EPSG code. Whatever a consumer reads the
//! CRS from (in-band metadata, a sidecar, a command-line flag), it reduces
//! it to an EPSG code and wraps it here; [`crate::Reprojector`] takes a
//! `SourceCrs`, never raw metadata or WKT strings.

/// Canonical source-CRS handle. Carries an EPSG code only. Keeping the
/// internal representation this narrow lets every downstream user (the
/// `Reprojector`, the parity oracle, a consumer's CRS override flag) work
/// uniformly without re-parsing format-specific payloads.
///
/// The newtype shape is deliberate: a bare `u16` would compose silently
/// with unrelated counts/sizes; wrapping it in a struct keeps the
/// type-system word load-bearing at API boundaries while staying simple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceCrs {
    pub epsg: u16,
}

impl SourceCrs {
    /// Construct from a raw EPSG code. Validation (is this code in the
    /// `crs-definitions` catalogue?) happens when `Reprojector::new` is
    /// called — we don't pre-check here so callers that just want to
    /// remember the code can do so without paying for the lookup.
    pub const fn new(epsg: u16) -> Self {
        Self { epsg }
    }

    /// EPSG:4978 — ECEF metres. The fixed reprojection target for 3D
    /// Tiles 1.1 `box` bounding volumes.
    pub const ECEF: Self = Self { epsg: 4978 };
}
