//! `tileforge-crs` — the error-agnostic CRS core shared by
//! `tileforge-pc` and `tileforge-mesh`.
//!
//! This crate is the CRS-handling core extracted from
//! `tileforge-pc-crs` (tileforge-pc ADR-004 / tileforge-mesh ADR-041
//! Phase 0). It carries the format-agnostic pieces:
//!
//! - [`SourceCrs`] — canonical source-CRS handle (a thin newtype around
//!   a `u16` EPSG code).
//! - [`Reprojector`] — wraps `proj4rs`; `new` takes a `SourceCrs`,
//!   `to_ecef` reprojects an `[x, y, z]` triple to ECEF metres
//!   (EPSG:4978, the 3D Tiles 1.1 `box` bounding-volume contract),
//!   `is_identity` is `true` iff the source EPSG is already 4978.
//! - [`parse_crs_string`] / [`parse_crs_string_epsg`] — parse a
//!   `EPSG:NNNNN` short form or OGC WKT body into an EPSG code, with a
//!   `vertical_stripped` flag for compound CRS.
//! - [`detect_crs_from_sidecar`] — resolve a `.prj` / `.qpj` sidecar
//!   next to an input file.
//! - [`extract_epsg_from_wkt`] — minimal OGC-WKT EPSG scanner.
//! - [`CrsHint`] / [`CrsResolution`] — caller-supplied CRS policy and
//!   the outcome of resolving it.
//! - [`EnuFrame`] — a local East-North-Up tangent frame at an ECEF origin:
//!   maps reprojected vertices into local metres and yields the 4×4
//!   `local→ECEF` matrix used as the 3D Tiles root `transform`
//!   (tileforge-mesh ADR-041 D2/D3). East→X, North→Y, Up→Z.
//!
//! **Error-agnostic.** The core does NOT depend on either consumer's
//! error crate; it reports a small [`CrsError`] `thiserror` enum, and
//! each consumer maps that onto its own schema at the call boundary
//! (`tileforge-pc` → `tileforge_pc_errors::ErrorKind::CrsError(String)`;
//! `tileforge-mesh` → its schema-v1 errors).
//!
//! **Vertical-datum policy lives in the consumer, not here.** Both
//! projects *strip* the vertical datum (PC per ADR-002; mesh because an
//! orthometric offset is a near-constant UI-correctable shift). The core
//! takes no policy: it reports a `vertical_stripped` flag and lets the
//! caller act (mesh warns; PC ignores).
//!
//! The PC-only LAS input adapter (`vlr`, `geokey`, `pre_pass`,
//! `extract_crs_from_las`, the `las` / `pasture-io` deps) stays in
//! `tileforge-pc-crs`, which re-exports this crate's surface.

mod crs_hint;
mod enu;
mod error;
mod reproject;
mod sidecar;
mod source_crs;
mod wkt;

pub use crs_hint::{CrsHint, CrsResolution};
pub use enu::{EnuFrame, ecef_to_geodetic_lonlat};
pub use error::CrsError;
pub use reproject::Reprojector;
pub use sidecar::{
    ParsedCrs, SidecarCrs, detect_crs_from_sidecar, parse_crs_string, parse_crs_string_epsg,
};
pub use source_crs::SourceCrs;
pub use wkt::{WktExtraction, extract_epsg_from_wkt};
