//! Sidecar CRS resolution for formats with no in-band CRS metadata
//! (PLY, XYZ/CSV) and the LAS cascade fallback.
//!
//! Convention: look for a sidecar file next to the input by replacing
//! the extension with `.prj` (GIS canonical) or `.qpj` (QGIS variant),
//! in either lower- or uppercase (`.PRJ` / `.QPJ` from legacy
//! ESRI/Windows tooling). Two body formats are accepted:
//!
//! - **OGC WKT** — full `PROJCS[...]` / `GEOGCS[...]` / `GEOCCS[...]`
//!   / `COMPD_CS[...]` string or its WKT2 equivalent.
//!   Reuses [`extract_epsg_from_wkt`](crate::extract_epsg_from_wkt).
//! - **Short-form `EPSG:NNNNN`** — single-line content produced by
//!   `gdalsrsinfo -o proj` and many lidar-export pipelines.
//!
//! Short-form is tried first (cheap). A body that starts with `EPSG:`
//! but carries an invalid code gets a short-form error; any other body
//! that fails surfaces the WKT-specific error.
//!
//! Returns:
//! - `Ok(Some(epsg))` — sidecar found and parsed.
//! - `Ok(None)` — neither sidecar exists.
//! - `Err(_)` — a sidecar exists but is unreadable / unparseable. Don't
//!   silently ignore: a malformed `.prj` next to a PLY is almost always
//!   a user mistake (renamed file, half-finished export).

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use crate::error::CrsError;
use crate::reproject::is_geotiff_sentinel;
use crate::wkt::{BYTE_ORDER_MARK, echo_fragment, extract_epsg};

/// Filename suffixes tried in order. `.prj` is the GIS canonical (ESRI,
/// GDAL, PDAL, lastools); `.qpj` is QGIS's WKT-2 variant. Both carry the
/// same OGC-WKT body for our purposes. Uppercase variants (`.PRJ` /
/// `.QPJ`) are also probed for legacy ESRI/Windows exports (F6); on a
/// case-sensitive filesystem those are distinct files. Lowercase is tried
/// first so it keeps precedence when both cases exist.
const SIDECAR_SUFFIXES: &[&str] = &[".prj", ".PRJ", ".qpj", ".QPJ"];

/// Largest sidecar body read, in bytes (1 MiB). A CRS definition is a few
/// kilobytes at most, so a larger file is refused before it is parsed.
const MAX_SIDECAR_BYTES: u64 = 1024 * 1024;

/// Outcome of parsing a CRS string. Carries the resolved EPSG plus
/// whether a vertical / compound component was present and dropped.
///
/// Surfacing `vertical_stripped` (rather than returning a bare `u16`)
/// lets each consumer act on it: `tileforge-mesh` warns the user that an
/// orthometric offset was discarded; `tileforge-pc` warns for LAS WKT
/// records and ignores it for sidecars (ADR-002 ellipsoidal-only). The core itself takes no policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedCrs {
    pub epsg: u16,
    /// `true` iff the input was a compound CRS (`COMPD_CS[...]` or
    /// `COMPOUNDCRS[...]`) whose vertical component was discarded during parsing.
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

/// Look for a sidecar `.prj`/`.qpj` (or uppercase `.PRJ`/`.QPJ`) next to
/// `input`. Suffixes are probed in the order `.prj`, `.PRJ`, `.qpj`,
/// `.QPJ`, and the first sidecar that **exists** is used: if it fails to
/// read or parse, the call returns that error and does not try the later
/// suffixes. Both suffix cases are probed so a `foo.PRJ` from legacy
/// ESRI/Windows tooling next to `foo.ply` is found on a case-sensitive
/// filesystem; the base filename stem must still match.
///
/// # Errors
///
/// - [`CrsError::Io`] when the sidecar cannot be read, is not UTF-8, or is
///   larger than 1 MiB (1,048,576 bytes). The size check reads at most one
///   byte past the limit.
/// - [`CrsError::Parse`] when the body is neither the `EPSG:NNNNN` short
///   form nor a supported WKT body.
pub fn detect_crs_from_sidecar(input: &Path) -> Result<Option<SidecarCrs>, CrsError> {
    for suffix in SIDECAR_SUFFIXES {
        let candidate = input.with_extension(suffix.trim_start_matches('.'));
        if !candidate.is_file() {
            continue;
        }
        let body = read_sidecar_bounded(&candidate)?;
        let parsed = match parse_crs_string(body.trim()) {
            Ok(p) => p,
            // A sidecar declaring a GeoTIFF sentinel (0 / 32767) is not a
            // parse failure — it is a valid "no standard CRS" declaration.
            // `parse_crs_string` now rejects sentinels (they are not
            // resolvable codes), but a sidecar's job is to report what the
            // file declares, so surface the sentinel as a resolved
            // `SidecarCrs`. The caller applies its own absence policy (e.g.
            // `is_geotiff_sentinel` → fall through to a local frame),
            // exactly as before the parser began rejecting sentinels.
            Err(CrsError::SentinelCode(epsg)) => ParsedCrs {
                epsg,
                vertical_stripped: false,
            },
            Err(e) => {
                return Err(CrsError::parse(format!(
                    "CRS sidecar {} is not parseable: {e}",
                    candidate.display()
                )));
            }
        };
        return Ok(Some(SidecarCrs {
            epsg: parsed.epsg,
            vertical_stripped: parsed.vertical_stripped,
            source: candidate,
        }));
    }
    Ok(None)
}

/// Read a sidecar body as UTF-8, refusing a file larger than
/// [`MAX_SIDECAR_BYTES`]. At most `MAX_SIDECAR_BYTES + 1` bytes are read or
/// allocated, whatever size the file reports.
fn read_sidecar_bounded(path: &Path) -> Result<String, CrsError> {
    let read_err = |e: std::io::Error| CrsError::io(path, format!("read CRS sidecar: {e}"));
    let file = File::open(path).map_err(read_err)?;
    let reported_len = file.metadata().map_or(0, |m| m.len());
    let first_capacity = usize::try_from(reported_len.min(MAX_SIDECAR_BYTES) + 1).unwrap_or(1);
    let mut bytes = Vec::with_capacity(first_capacity);
    let mut reader = file.take(MAX_SIDECAR_BYTES + 1);
    let mut chunk = [0u8; 8192];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(read_err(e)),
        };
        // `reserve_exact` keeps the capacity at the bytes read so far, so a
        // file that reports a wrong length still cannot raise it past the cap.
        bytes.reserve_exact(n);
        bytes.extend_from_slice(&chunk[..n]);
    }
    if bytes.len() as u64 > MAX_SIDECAR_BYTES {
        return Err(CrsError::io(
            path,
            format!(
                "CRS sidecar is larger than the {MAX_SIDECAR_BYTES}-byte limit; \
                 a CRS definition is a few kilobytes, so this file is not read"
            ),
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        read_err(std::io::Error::new(
            ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        ))
    })
}

/// Parse a CRS string into a [`ParsedCrs`] (EPSG + vertical-stripped
/// flag). Accepts two forms:
///
/// - **`EPSG:NNNNN`** short form (GDAL `gdalsrsinfo -o proj` style).
///   Case-insensitive on the `EPSG` prefix; tolerates surrounding
///   whitespace. Never carries a vertical component.
/// - **OGC WKT 1 or WKT 2** — `PROJCS[...]`, `GEOGCS[...]`, `GEOCCS[...]`,
///   `COMPD_CS[...]`, `PROJCRS[...]`, `GEOGCRS[...]`, `GEODCRS[...]`, or
///   `COMPOUNDCRS[...]`. Body parsed via
///   [`extract_epsg_from_wkt`](crate::extract_epsg_from_wkt); a
///   compound CRS sets `vertical_stripped = true`.
///
/// Used by both [`detect_crs_from_sidecar`] (sidecar `.prj`/`.qpj`)
/// and the E57 reader's `coordinateMetadata` ingest. Callers should
/// pass an already-trimmed string. One leading UTF-8 byte order mark
/// (U+FEFF), as Windows Notepad writes it, is ignored together with the
/// whitespace after it; a byte order mark at any other position is not
/// special. Errors with a single message naming both supported forms;
/// callers wrap with source-specific context.
///
/// # Sentinel rejection
///
/// A body that resolves to a GeoTIFF reserved sentinel (`EPSG:0` /
/// `EPSG:32767`, or a WKT whose authority code is one of those) is
/// **rejected** with [`CrsError::SentinelCode`] — a sentinel is not a
/// resolvable CRS code but a declaration of "no standard CRS". Consumers
/// that treat a sentinel as *absence* (fall through to a local frame) should
/// match that variant and/or pre-check with
/// [`is_geotiff_sentinel`](crate::is_geotiff_sentinel), rather than feed the
/// value to [`Reprojector::new`](crate::Reprojector::new) / the proj4
/// catalogue.
pub fn parse_crs_string(body: &str) -> Result<ParsedCrs, CrsError> {
    let body = match body.strip_prefix(BYTE_ORDER_MARK) {
        Some(rest) => rest.trim_start(),
        None => body,
    };
    if let Some(short_form) = parse_short_form_epsg(body) {
        let epsg = short_form?;
        if is_geotiff_sentinel(epsg) {
            return Err(CrsError::sentinel_code(epsg));
        }
        return Ok(ParsedCrs {
            epsg,
            vertical_stripped: false,
        });
    }
    let parsed = extract_epsg(body).map(|e| ParsedCrs {
        epsg: e.epsg,
        vertical_stripped: e.vertical_stripped,
    });
    match parsed {
        Ok(p) if is_geotiff_sentinel(p.epsg) => Err(CrsError::sentinel_code(p.epsg)),
        Ok(p) => Ok(p),
        Err(e) => Err(CrsError::parse(format!(
            "expected `EPSG:NNNNN` short form or OGC WKT1 \
             `PROJCS`/`GEOGCS`/`GEOCCS`/`COMPD_CS` or WKT2 \
             `PROJCRS`/`GEOGCRS`/`GEODCRS`/`COMPOUNDCRS`; WKT parse error: {e}"
        ))),
    }
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
/// internal space is fine). Returns `None` if the body does not start
/// with `EPSG:` (the caller falls through to WKT), and `Some(Err(_))` if
/// it does but the code is not a `u16`.
fn parse_short_form_epsg(body: &str) -> Option<Result<u16, CrsError>> {
    let (prefix, rest) = body.split_once(':')?;
    if !prefix.trim().eq_ignore_ascii_case("EPSG") {
        return None;
    }
    let code_text = rest.trim();
    Some(code_text.parse::<u16>().map_err(|_| {
        CrsError::parse(format!(
            "`EPSG:NNNNN` short form has an invalid code {:?}; the code must be an \
             integer from 0 to 65535",
            echo_fragment(code_text)
        ))
    }))
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
    fn oversized_sidecar_fails_with_size_limit_error() {
        let ply = write_tmp("oversized.ply", "ply\n");
        let prj = ply.with_extension("prj");
        let f = std::fs::File::create(&prj).unwrap();
        f.set_len(8 * 1024 * 1024).unwrap();
        let err = detect_crs_from_sidecar(&ply).unwrap_err();
        let CrsError::Io { path, detail } = &err else {
            panic!("want Io got {err:?}");
        };
        assert_eq!(path, &prj.display().to_string());
        assert!(
            detail.contains(&MAX_SIDECAR_BYTES.to_string()),
            "got: {detail}"
        );
        assert!(detail.contains("limit"), "got: {detail}");
    }

    #[test]
    fn sidecar_at_size_limit_is_read() {
        let ply = write_tmp("at-limit.ply", "ply\n");
        let prj = ply.with_extension("prj");
        let limit = usize::try_from(MAX_SIDECAR_BYTES).unwrap();
        let mut body = KINGSTON_WKT.to_string();
        body.push_str(&" ".repeat(limit - body.len()));
        assert_eq!(body.len(), limit);
        std::fs::write(&prj, &body).unwrap();
        assert_eq!(detect_crs_from_sidecar(&ply).unwrap().unwrap().epsg, 32617);

        body.push(' ');
        std::fs::write(&prj, &body).unwrap();
        assert!(matches!(
            detect_crs_from_sidecar(&ply).unwrap_err(),
            CrsError::Io { .. }
        ));
    }

    #[test]
    fn non_utf8_sidecar_is_io_error() {
        let ply = write_tmp("latin1.ply", "ply\n");
        let prj = ply.with_extension("prj");
        std::fs::write(&prj, b"EPSG:\xff32617").unwrap();
        let err = detect_crs_from_sidecar(&ply).unwrap_err();
        let CrsError::Io { detail, .. } = &err else {
            panic!("want Io got {err:?}");
        };
        assert_eq!(
            detail,
            "read CRS sidecar: stream did not contain valid UTF-8"
        );
    }

    #[test]
    fn uppercase_prj_sidecar_is_found() {
        // F6: legacy ESRI/Windows tooling emits `FOO.PRJ`. On case-sensitive
        // Linux filesystems the old lowercase-only probe missed it and
        // returned Ok(None), silently dropping georeferencing. We now also
        // try the uppercase suffix variant.
        let ply = write_tmp("upper.ply", "ply\n");
        let prj = ply.with_extension("PRJ");
        std::fs::write(&prj, KINGSTON_WKT).unwrap();
        let r = detect_crs_from_sidecar(&ply).unwrap();
        assert!(r.is_some(), "uppercase .PRJ sidecar must be found");
        let r = r.unwrap();
        assert_eq!(r.epsg, 32617);
        assert_eq!(r.source, prj);
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
    fn short_form_with_invalid_code_reports_the_short_form() {
        for (body, shown) in [
            ("EPSG:65536", "\"65536\""),
            ("EPSG:", "\"\""),
            ("EPSG:-1", "\"-1\""),
            ("EPSG:32617 extra", "\"32617 extra\""),
            ("epsg : abc", "\"abc\""),
        ] {
            let err = parse_crs_string(body).unwrap_err();
            let CrsError::Parse(msg) = &err else {
                panic!("want Parse got {err:?}");
            };
            assert!(
                msg.contains("short form has an invalid code"),
                "{body}: {msg}"
            );
            assert!(msg.contains(shown), "{body}: {msg}");
            assert!(!msg.contains("not followed by"), "{body}: {msg}");
        }
    }

    #[test]
    fn short_form_invalid_code_text_is_truncated() {
        let body = format!("EPSG:{}", "9".repeat(10_000));
        let msg = parse_crs_string(&body).unwrap_err().to_string();
        assert!(msg.len() < 400, "len {}", msg.len());
    }

    #[test]
    fn short_form_sign_and_leading_zero_still_parse() {
        assert_eq!(parse_crs_string("EPSG:+32617").unwrap().epsg, 32617);
        assert_eq!(parse_crs_string("EPSG:032617").unwrap().epsg, 32617);
    }

    #[test]
    fn combined_error_names_wkt1_and_wkt2_roots() {
        let msg = parse_crs_string("this is not WKT").unwrap_err().to_string();
        for root in ["PROJCS", "COMPD_CS", "PROJCRS", "COMPOUNDCRS", "EPSG:NNNNN"] {
            assert!(msg.contains(root), "missing {root}: {msg}");
        }
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

    #[test]
    fn parse_crs_string_rejects_geotiff_sentinels_short_form() {
        // F3: `EPSG:0` / `EPSG:32767` are GeoTIFF "no CRS" sentinels, not
        // resolvable codes. The parser must reject them with the structured
        // `SentinelCode` variant (carrying the value) rather than returning
        // `Ok(0)` / `Ok(32767)`.
        for code in [0u16, 32767] {
            let err = parse_crs_string(&format!("EPSG:{code}")).unwrap_err();
            assert_eq!(
                err,
                CrsError::SentinelCode(code),
                "EPSG:{code} must be rejected as a sentinel"
            );
        }
    }

    #[test]
    fn parse_crs_string_epsg_rejects_geotiff_sentinels() {
        // The bare-code convenience wrapper inherits the rejection.
        for code in [0u16, 32767] {
            let err = parse_crs_string_epsg(&format!("EPSG:{code}")).unwrap_err();
            assert_eq!(err, CrsError::SentinelCode(code));
        }
    }

    #[test]
    fn parse_crs_string_still_accepts_private_range_above_sentinel() {
        // 32768.. is the implementation-defined user range, NOT a sentinel;
        // it must still parse (and later fail as an unknown EPSG, not here).
        let p = parse_crs_string("EPSG:32768").unwrap();
        assert_eq!(p.epsg, 32768);
    }

    /// Bodies that start with a UTF-8 byte order mark, as Windows Notepad
    /// saves them, and the EPSG code each one declares.
    const BOM_BODIES: &[(&str, u16)] = &[
        ("\u{feff}EPSG:32614", 32614),
        ("\u{feff}  epsg : 26917\r\n", 26917),
        ("\u{feff}\r\nEPSG:32617", 32617),
    ];

    #[test]
    fn parse_crs_string_ignores_a_leading_byte_order_mark() {
        for &(body, epsg) in BOM_BODIES {
            assert_eq!(
                parse_crs_string(body.trim()).unwrap().epsg,
                epsg,
                "{body:?}"
            );
        }
        for body in [
            format!("\u{feff}{KINGSTON_WKT}"),
            format!("\u{feff}\r\n{KINGSTON_WKT}"),
            format!("\u{feff} \n\t{KINGSTON_WKT}"),
        ] {
            let p = parse_crs_string(body.trim()).unwrap();
            assert_eq!(p.epsg, 32617, "{:?}", &body[..8]);
            assert!(!p.vertical_stripped);
        }
        let p = parse_crs_string(&format!("\u{feff}{COMPD_WKT}")).unwrap();
        assert_eq!(p.epsg, 32617);
        assert!(p.vertical_stripped);
    }

    #[test]
    fn byte_order_mark_is_ignored_only_at_the_start() {
        for body in [
            "\u{feff}\u{feff}EPSG:32617".to_string(),
            " \u{feff}EPSG:32617".to_string(),
            "EPSG:\u{feff}32617".to_string(),
            format!("\u{feff}\u{feff}{KINGSTON_WKT}"),
            format!(" \u{feff}{KINGSTON_WKT}"),
        ] {
            assert!(parse_crs_string(&body).is_err(), "{body:?}");
        }
    }

    #[test]
    fn sidecar_with_a_leading_byte_order_mark_resolves() {
        let mut cases: Vec<(String, u16)> = BOM_BODIES
            .iter()
            .map(|&(body, epsg)| (body.to_string(), epsg))
            .collect();
        cases.push((format!("\u{feff}{KINGSTON_WKT}"), 32617));
        cases.push((format!("\u{feff}\r\n{KINGSTON_WKT}\r\n"), 32617));
        for (n, (body, epsg)) in cases.iter().enumerate() {
            let ply = write_tmp(&format!("bom-{n}.ply"), "ply\n");
            let prj = ply.with_extension("prj");
            std::fs::write(&prj, body).unwrap();
            let r = detect_crs_from_sidecar(&ply)
                .unwrap_or_else(|e| panic!("{body:?}: {e}"))
                .unwrap();
            assert_eq!(r.epsg, *epsg, "{body:?}");
            assert!(!r.vertical_stripped);
        }
    }

    #[test]
    fn sidecar_declaring_sentinel_resolves_to_sentinel_not_parse_error() {
        // A `.prj` carrying `EPSG:0` (or 32767) is a valid "no CRS"
        // declaration, not a malformed sidecar: `detect_crs_from_sidecar`
        // surfaces the sentinel as a resolved `SidecarCrs` (Ok(Some(..))) so
        // the caller can apply its own absence policy. It must NOT become a
        // `CrsError::parse` "not parseable" error.
        for code in [0u16, 32767] {
            let ply = write_tmp(&format!("sentinel-{code}.ply"), "ply\n");
            let prj = ply.with_extension("prj");
            std::fs::write(&prj, format!("EPSG:{code}")).unwrap();
            let r = detect_crs_from_sidecar(&ply).unwrap().unwrap();
            assert_eq!(r.epsg, code);
            assert!(is_geotiff_sentinel(r.epsg));
            assert!(!r.vertical_stripped);
        }
    }
}
