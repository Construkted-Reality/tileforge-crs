//! Sidecar CRS resolution for formats with no in-band CRS metadata
//! (PLY, XYZ/CSV) and the LAS cascade fallback.
//!
//! Convention: look for a sidecar file next to the input by replacing
//! the extension with `.prj` (GIS canonical) or `.qpj` (QGIS variant).
//! Two body formats are accepted:
//!
//! - **OGC WKT** — full `PROJCS[...]` / `GEOGCS[...]` / `GEOCCS[...]`
//!   / `COMPD_CS[...]` string, same format as a LAS-1.4 WKT VLR.
//!   Reuses [`extract_epsg_from_wkt`].
//! - **Short-form `EPSG:NNNNN`** — single-line content produced by
//!   `gdalsrsinfo -o proj` and many lidar-export pipelines.
//!
//! Short-form is tried first (cheap) so a malformed WKT body still
//! surfaces the WKT-specific error.
//!
//! Returns:
//! - `Ok(Some(epsg))` — sidecar found and parsed.
//! - `Ok(None)` — neither sidecar exists.
//! - `Err(_)` — a sidecar exists but is unreadable / unparseable. Don't
//!   silently ignore: a malformed `.prj` next to a PLY is almost always
//!   a user mistake (renamed file, half-finished export).

use std::path::Path;

use crate::error::CrsError;
use crate::wkt::extract_epsg_from_wkt;

/// Filename suffixes (lowercase) tried in order. `.prj` is the GIS
/// canonical (ESRI, GDAL, PDAL, lastools); `.qpj` is QGIS's WKT-2
/// variant. Both carry the same OGC-WKT body for our purposes.
const SIDECAR_SUFFIXES: &[&str] = &[".prj", ".qpj"];

/// Outcome of parsing a CRS string. Carries the resolved EPSG plus
/// whether a vertical / compound component was present and dropped.
///
/// Surfacing `vertical_stripped` (rather than returning a bare `u16`)
/// lets each consumer act on it: `tileforge-mesh` warns the user that an
/// orthometric offset was discarded; `tileforge-pc` ignores it (ADR-002
/// ellipsoidal-only). The core itself takes no policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedCrs {
    pub epsg: u16,
    /// `true` iff the input was a `COMPD_CS[...]` whose vertical
    /// component was discarded during parsing.
    pub vertical_stripped: bool,
}

/// Result of a successful sidecar resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarCrs {
    pub epsg: u16,
    /// `true` iff a `COMPD_CS[...]` vertical component was stripped while
    /// parsing the sidecar body. Short-form `EPSG:NNNNN` sidecars never
    /// carry a vertical, so this is always `false` for them.
    pub vertical_stripped: bool,
    /// Path to the sidecar that produced this EPSG. Logged at info
    /// level by callers so operators can see which file was consumed.
    pub source: std::path::PathBuf,
}

/// Look for a sidecar `.prj` or `.qpj` next to `input`. Returns the
/// first sidecar that parses cleanly. The lookup is case-insensitive
/// in the suffix only — Linux file systems are case-sensitive, so
/// `Foo.PRJ` next to `foo.ply` is intentionally not found (the user
/// would write `foo.prj`).
pub fn detect_crs_from_sidecar(input: &Path) -> Result<Option<SidecarCrs>, CrsError> {
    for suffix in SIDECAR_SUFFIXES {
        let candidate = input.with_extension(suffix.trim_start_matches('.'));
        if !candidate.is_file() {
            continue;
        }
        let body = std::fs::read_to_string(&candidate)
            .map_err(|e| CrsError::io(&candidate, format!("read CRS sidecar: {e}")))?;
        let parsed = parse_crs_string(body.trim()).map_err(|e| {
            CrsError::parse(format!(
                "CRS sidecar {} is not parseable: {e}",
                candidate.display()
            ))
        })?;
        return Ok(Some(SidecarCrs {
            epsg: parsed.epsg,
            vertical_stripped: parsed.vertical_stripped,
            source: candidate,
        }));
    }
    Ok(None)
}

/// Parse a CRS string into a [`ParsedCrs`] (EPSG + vertical-stripped
/// flag). Accepts two forms:
///
/// - **`EPSG:NNNNN`** short form (GDAL `gdalsrsinfo -o proj` style).
///   Case-insensitive on the `EPSG` prefix; tolerates surrounding
///   whitespace. Never carries a vertical component.
/// - **OGC WKT 1** — `PROJCS[...]`, `GEOGCS[...]`, `GEOCCS[...]`, or
///   `COMPD_CS[...]`. Body parsed via [`extract_epsg_from_wkt`]; a
///   `COMPD_CS` sets `vertical_stripped = true`.
///
/// Used by both [`detect_crs_from_sidecar`] (sidecar `.prj`/`.qpj`)
/// and the E57 reader's `coordinateMetadata` ingest. Callers should
/// pass an already-trimmed string. Errors with a single message naming
/// both supported forms; callers wrap with source-specific context.
pub fn parse_crs_string(body: &str) -> Result<ParsedCrs, CrsError> {
    if let Some(epsg) = parse_short_form_epsg(body) {
        return Ok(ParsedCrs {
            epsg,
            vertical_stripped: false,
        });
    }
    extract_epsg_from_wkt(body)
        .map(|e| ParsedCrs {
            epsg: e.epsg,
            vertical_stripped: e.vertical_stripped,
        })
        .map_err(|e| {
            CrsError::parse(format!(
                "expected `EPSG:NNNNN` short form or OGC WKT \
                 `PROJCS[...]` / `GEOGCS[...]` / `GEOCCS[...]` / \
                 `COMPD_CS[...]`; WKT parse error: {e}"
            ))
        })
}

/// Convenience wrapper over [`parse_crs_string`] returning just the
/// EPSG code, discarding the `vertical_stripped` flag. For callers that
/// don't care whether a vertical datum was dropped.
pub fn parse_crs_string_epsg(body: &str) -> Result<u16, CrsError> {
    parse_crs_string(body).map(|p| p.epsg)
}

/// Parse the GDAL-canonical short form `EPSG:NNNNN`. Case-insensitive
/// on the `EPSG` prefix; tolerates whitespace around the colon and a
/// trailing newline (already trimmed by the caller, but a stray
/// internal space is fine). Returns `None` if the body doesn't match
/// the short form — caller falls through to WKT.
fn parse_short_form_epsg(body: &str) -> Option<u16> {
    let (prefix, rest) = body.split_once(':')?;
    if !prefix.trim().eq_ignore_ascii_case("EPSG") {
        return None;
    }
    rest.trim().parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tileforge-crs-sidecar-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    const KINGSTON_WKT: &str = include_str!("../tests/fixtures/kingston-rd.wkt");
    const COMPD_WKT: &str = include_str!("../tests/fixtures/compd-cs-with-vert.wkt");

    #[test]
    fn no_sidecar_returns_none() {
        let p = write_tmp("solo.ply", "ply\n");
        assert_eq!(detect_crs_from_sidecar(&p).unwrap(), None);
    }

    #[test]
    fn prj_sidecar_resolves() {
        let ply = write_tmp("with-prj.ply", "ply\n");
        let prj = ply.with_extension("prj");
        std::fs::write(&prj, KINGSTON_WKT).unwrap();
        let r = detect_crs_from_sidecar(&ply).unwrap().unwrap();
        assert_eq!(r.epsg, 32617);
        assert!(!r.vertical_stripped);
        assert_eq!(r.source, prj);
    }

    #[test]
    fn qpj_sidecar_resolves_when_no_prj() {
        let ply = write_tmp("with-qpj.ply", "ply\n");
        let qpj = ply.with_extension("qpj");
        std::fs::write(&qpj, KINGSTON_WKT).unwrap();
        let r = detect_crs_from_sidecar(&ply).unwrap().unwrap();
        assert_eq!(r.epsg, 32617);
        assert_eq!(r.source, qpj);
    }

    #[test]
    fn prj_takes_precedence_over_qpj() {
        // Different EPSG in each so we can tell which one was read.
        let ply = write_tmp("both.ply", "ply\n");
        let prj = ply.with_extension("prj");
        let qpj = ply.with_extension("qpj");
        std::fs::write(&prj, KINGSTON_WKT).unwrap();
        std::fs::write(&qpj, include_str!("../tests/fixtures/fabregualta.wkt")).unwrap();
        let r = detect_crs_from_sidecar(&ply).unwrap().unwrap();
        assert_eq!(r.epsg, 32617, ".prj must win over .qpj");
        assert_eq!(r.source.extension().unwrap(), "prj");
    }

    #[test]
    fn compd_cs_sidecar_reports_vertical_stripped() {
        let ply = write_tmp("compd.ply", "ply\n");
        let prj = ply.with_extension("prj");
        std::fs::write(&prj, COMPD_WKT).unwrap();
        let r = detect_crs_from_sidecar(&ply).unwrap().unwrap();
        assert_eq!(r.epsg, 32617);
        assert!(
            r.vertical_stripped,
            "COMPD_CS sidecar must report vertical_stripped = true"
        );
    }

    #[test]
    fn malformed_sidecar_errors_loudly() {
        let ply = write_tmp("bad.ply", "ply\n");
        let prj = ply.with_extension("prj");
        std::fs::write(&prj, "this is not WKT").unwrap();
        let err = detect_crs_from_sidecar(&ply).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("sidecar"), "got: {msg}");
        assert!(msg.contains(".prj"), "got: {msg}");
        // Error names both supported formats so the user knows the
        // options.
        assert!(msg.contains("EPSG:NNNNN"), "got: {msg}");
        assert!(msg.contains("WKT"), "got: {msg}");
    }

    #[test]
    fn short_form_epsg_sidecar_resolves() {
        // GDAL `gdalsrsinfo -o proj` output.
        let ply = write_tmp("short-form.ply", "ply\n");
        let prj = ply.with_extension("prj");
        std::fs::write(&prj, "EPSG:32614").unwrap();
        let r = detect_crs_from_sidecar(&ply).unwrap().unwrap();
        assert_eq!(r.epsg, 32614);
        assert!(!r.vertical_stripped);
    }

    #[test]
    fn short_form_case_insensitive_and_whitespace_tolerant() {
        let ply = write_tmp("loose.ply", "ply\n");
        let prj = ply.with_extension("prj");
        std::fs::write(&prj, "  epsg : 26917 \n").unwrap();
        let r = detect_crs_from_sidecar(&ply).unwrap().unwrap();
        assert_eq!(r.epsg, 26917);
    }

    #[test]
    fn non_epsg_short_string_falls_through_to_wkt_error() {
        // "AUTO:42001" is OGC AUTO CRS — we don't support it; should
        // hit the WKT parser and surface the WKT-specific error.
        let ply = write_tmp("auto.ply", "ply\n");
        let prj = ply.with_extension("prj");
        std::fs::write(&prj, "AUTO:42001").unwrap();
        let err = detect_crs_from_sidecar(&ply).unwrap_err();
        assert!(err.to_string().contains("WKT"), "got: {err}");
    }

    #[test]
    fn parse_crs_string_short_form_no_vertical() {
        let p = parse_crs_string("EPSG:32617").unwrap();
        assert_eq!(p.epsg, 32617);
        assert!(!p.vertical_stripped);
    }

    #[test]
    fn parse_crs_string_compd_cs_sets_vertical_stripped() {
        let p = parse_crs_string(COMPD_WKT).unwrap();
        assert_eq!(p.epsg, 32617);
        assert!(p.vertical_stripped);
    }

    #[test]
    fn parse_crs_string_epsg_convenience_returns_bare_code() {
        assert_eq!(parse_crs_string_epsg(KINGSTON_WKT).unwrap(), 32617);
        assert_eq!(parse_crs_string_epsg("EPSG:26917").unwrap(), 26917);
    }
}
