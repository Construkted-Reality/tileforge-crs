//! Minimal OGC WKT scanner — extracts the outermost EPSG `AUTHORITY`
//! clause from a CRS WKT string.
//!
//! Not a full WKT parser. The strategy is bracket-balanced substring
//! scanning, which is sufficient for the Phase 1.1 deliverable: pull the
//! EPSG code out of a real-world LAS-1.4 WKT VLR (which always carries
//! an `AUTHORITY["EPSG","NNNN"]` clause when produced by PDAL, lastools,
//! QGIS, or GDAL).
//!
//! Supported root blocks: `PROJCS`, `GEOGCS`, `GEOCCS`, and `COMPD_CS`.
//! For `COMPD_CS`, the scanner descends into the first horizontal
//! subblock (`PROJCS` / `GEOGCS` / `GEOCCS`) and reports the vertical
//! component as stripped via `WktExtraction::vertical_stripped` — the
//! caller decides what to do (PC ignores, mesh warns).

use crate::error::CrsError;

/// Result of a successful WKT scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WktExtraction {
    pub epsg: u16,
    /// `true` iff the input was a `COMPD_CS[...]` and the vertical
    /// component was discarded.
    pub vertical_stripped: bool,
}

/// Extract the EPSG code from a CRS WKT string.
pub fn extract_epsg_from_wkt(wkt: &str) -> Result<WktExtraction, CrsError> {
    let bytes = wkt.as_bytes();
    let after_ws = skip_whitespace(bytes, 0);
    let (id, after_id) = read_identifier(bytes, after_ws).ok_or_else(|| {
        CrsError::parse("WKT VLR is empty or has no leading identifier".to_string())
    })?;
    let id_upper = id.to_ascii_uppercase();
    let after_open = skip_whitespace(bytes, after_id);
    if after_open >= bytes.len() || bytes[after_open] != b'[' {
        return Err(CrsError::parse(format!(
            "WKT VLR identifier '{id}' is not followed by '['"
        )));
    }
    let close_idx = find_matching_bracket(bytes, after_open).ok_or_else(|| {
        CrsError::parse(format!("WKT VLR identifier '{id}' has unbalanced brackets"))
    })?;
    let body = &wkt[after_open + 1..close_idx];

    match id_upper.as_str() {
        "COMPD_CS" => {
            let horizontal = find_first_horizontal_subblock(body).ok_or_else(|| {
                CrsError::parse(
                    "COMPD_CS contains no PROJCS/GEOGCS/GEOCCS subblock to extract EPSG from"
                        .to_string(),
                )
            })?;
            let inner = extract_epsg_from_wkt(horizontal)?;
            Ok(WktExtraction {
                epsg: inner.epsg,
                vertical_stripped: true,
            })
        }
        "PROJCS" | "GEOGCS" | "GEOCCS" => {
            let epsg = parse_epsg_authority_in_body(body)?;
            Ok(WktExtraction {
                epsg,
                vertical_stripped: false,
            })
        }
        _ => Err(CrsError::parse(format!(
            "WKT VLR root is unexpected '{id}' (want PROJCS / GEOGCS / GEOCCS / COMPD_CS)"
        ))),
    }
}

fn skip_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn read_identifier(bytes: &[u8], start: usize) -> Option<(&str, usize)> {
    let mut end = start;
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_alphanumeric() || c == b'_' {
            end += 1;
        } else {
            break;
        }
    }
    if end == start {
        return None;
    }
    // Safe: identifier characters are all ASCII.
    Some((std::str::from_utf8(&bytes[start..end]).unwrap(), end))
}

/// Given `bytes[open_idx] == b'['`, return the index of the matching
/// closing `b']'`. Respects WKT-quoted strings (`"..."`) so brackets
/// inside CRS names don't mis-count.
fn find_matching_bracket(bytes: &[u8], open_idx: usize) -> Option<usize> {
    debug_assert_eq!(bytes[open_idx], b'[');
    let mut depth: i32 = 0;
    let mut i = open_idx;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'"' {
                // WKT escapes a literal quote as `""`. Peek ahead.
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    i += 2;
                    continue;
                }
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Walk the tokens of `body` (which is the contents of a COMPD_CS block,
/// excluding its outer brackets). Return the substring of the first
/// child block whose identifier is one of PROJCS/GEOGCS/GEOCCS.
fn find_first_horizontal_subblock(body: &str) -> Option<&str> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            // Skip quoted string.
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'"' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphanumeric() || c == b'_' {
            let (id, after_id) = read_identifier(bytes, i)?;
            let after_ws = skip_whitespace(bytes, after_id);
            if after_ws < bytes.len() && bytes[after_ws] == b'[' {
                let close = find_matching_bracket(bytes, after_ws)?;
                let id_upper = id.to_ascii_uppercase();
                if matches!(id_upper.as_str(), "PROJCS" | "GEOGCS" | "GEOCCS") {
                    return Some(&body[i..=close]);
                }
                // Skip past this non-horizontal block (e.g. VERT_CS,
                // AUTHORITY) and continue scanning.
                i = close + 1;
                continue;
            }
            i = after_id;
            continue;
        }
        i += 1;
    }
    None
}

/// Scan `body` (the contents of a PROJCS/GEOGCS/GEOCCS block, excluding
/// its outer brackets) for an `AUTHORITY[...]` at depth 0. Return the
/// EPSG u16 code from inside it.
fn parse_epsg_authority_in_body(body: &str) -> Result<u16, CrsError> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'"' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphanumeric() || c == b'_' {
            let Some((id, after_id)) = read_identifier(bytes, i) else {
                i += 1;
                continue;
            };
            let after_ws = skip_whitespace(bytes, after_id);
            if after_ws < bytes.len() && bytes[after_ws] == b'[' {
                let Some(close) = find_matching_bracket(bytes, after_ws) else {
                    return Err(CrsError::parse(
                        "unbalanced brackets while scanning for AUTHORITY".to_string(),
                    ));
                };
                if id.eq_ignore_ascii_case("AUTHORITY") {
                    let inner = &body[after_ws + 1..close];
                    return parse_authority_args(inner);
                }
                i = close + 1;
                continue;
            }
            i = after_id;
            continue;
        }
        i += 1;
    }
    Err(CrsError::parse(
        "WKT VLR has no EPSG AUTHORITY clause; pre-process with `pdal translate` to inject one"
            .to_string(),
    ))
}

/// Parse the contents of an AUTHORITY block: `"EPSG","NNNN"`.
fn parse_authority_args(inner: &str) -> Result<u16, CrsError> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return Err(CrsError::parse(format!(
            "AUTHORITY clause must have two arguments, got {} ({inner:?})",
            parts.len()
        )));
    }
    let authority = strip_quotes(parts[0])?;
    let code_str = strip_quotes(parts[1])?;
    if !authority.eq_ignore_ascii_case("EPSG") {
        return Err(CrsError::parse(format!(
            "WKT VLR uses non-EPSG authority '{authority}'; only EPSG is supported"
        )));
    }
    code_str.parse::<u16>().map_err(|e| {
        CrsError::parse(format!(
            "AUTHORITY code is not a valid EPSG u16: {code_str:?}: {e}"
        ))
    })
}

fn strip_quotes(s: &str) -> Result<&str, CrsError> {
    let s = s.trim();
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return Err(CrsError::parse(format!(
            "AUTHORITY argument is not quoted: {s:?}"
        )));
    }
    Ok(&s[1..s.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINGSTON_WKT: &str = include_str!("../tests/fixtures/kingston-rd.wkt");
    const FABRE_WKT: &str = include_str!("../tests/fixtures/fabregualta.wkt");
    const COMPD_WKT: &str = include_str!("../tests/fixtures/compd-cs-with-vert.wkt");
    const ECEF_WKT: &str = include_str!("../tests/fixtures/epsg-4978.wkt");

    #[test]
    fn projcs_kingston_yields_32617_no_vertical() {
        let r = extract_epsg_from_wkt(KINGSTON_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 32617,
                vertical_stripped: false
            }
        );
    }

    #[test]
    fn projcs_fabregualta_yields_32719() {
        let r = extract_epsg_from_wkt(FABRE_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 32719,
                vertical_stripped: false
            }
        );
    }

    #[test]
    fn geoccs_ecef_wkt_yields_4978() {
        let r = extract_epsg_from_wkt(ECEF_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 4978,
                vertical_stripped: false
            }
        );
    }

    #[test]
    fn compd_cs_returns_horizontal_epsg_and_strips_vertical() {
        let r = extract_epsg_from_wkt(COMPD_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 32617,
                vertical_stripped: true
            }
        );
    }

    #[test]
    fn projcs_with_no_authority_is_crs_error() {
        let wkt = r#"PROJCS["fake", GEOGCS["wgs",DATUM["d",SPHEROID["s",6378137,298.257]]], PROJECTION["tm"], UNIT["m",1]]"#;
        let err = extract_epsg_from_wkt(wkt).unwrap_err();
        let CrsError::Parse(msg) = &err else {
            panic!("want Parse got {err:?}");
        };
        assert!(msg.to_lowercase().contains("authority"), "msg: {msg}");
    }

    #[test]
    fn projcs_with_non_epsg_authority_is_crs_error() {
        let wkt = r#"PROJCS["esri thing", AUTHORITY["ESRI","102100"]]"#;
        let err = extract_epsg_from_wkt(wkt).unwrap_err();
        let CrsError::Parse(msg) = &err else {
            panic!("want Parse got {err:?}");
        };
        assert!(msg.to_lowercase().contains("esri"), "msg: {msg}");
    }

    #[test]
    fn empty_string_is_crs_error() {
        assert!(matches!(
            extract_epsg_from_wkt("").unwrap_err(),
            CrsError::Parse(_)
        ));
    }

    #[test]
    fn unbalanced_brackets_is_crs_error() {
        assert!(matches!(
            extract_epsg_from_wkt(r#"PROJCS["x", AUTHORITY["EPSG","32617""#).unwrap_err(),
            CrsError::Parse(_)
        ));
    }

    #[test]
    fn unknown_root_identifier_is_crs_error() {
        assert!(matches!(
            extract_epsg_from_wkt(r#"VERT_CS["only vertical", AUTHORITY["EPSG","5773"]]"#)
                .unwrap_err(),
            CrsError::Parse(_)
        ));
    }

    #[test]
    fn whitespace_and_lowercase_are_tolerated() {
        let wkt = r#"
            projcs [ "WGS 84 / UTM zone 17N",
                geogcs [ "WGS 84",
                    authority [ "EPSG", "4326" ] ],
                authority [ "EPSG" , "32617" ]
            ]
        "#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 32617);
    }

    #[test]
    fn quoted_brackets_in_name_do_not_mis_count() {
        // CRS name happens to contain a `]` inside a quoted string;
        // bracket counting must not treat it as a closer.
        let wkt = r#"PROJCS["weird]name", AUTHORITY["EPSG","32617"]]"#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 32617);
    }
}
