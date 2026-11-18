//! Minimal OGC WKT scanner — extracts the outermost EPSG `AUTHORITY`
//! clause from a CRS WKT string.
//!
//! Not a full WKT parser. The strategy is bracket-balanced substring
//! scanning, which is enough to pull the EPSG code out of a CRS WKT from
//! any source (a LAS 1.4 WKT record, a `.prj`/`.qpj` sidecar, E57
//! metadata). PDAL, lastools, QGIS and GDAL write an
//! `AUTHORITY["EPSG","NNNN"]` clause; ESRI-style `.prj` files usually do
//! not, and such a WKT fails because this scanner resolves a CRS by its
//! EPSG code only. Square brackets are the only supported delimiters.
//!
//! Supported root blocks — both **WKT1** and **WKT2** (LAS 1.4 producers
//! increasingly emit WKT2):
//! - WKT1: `PROJCS`, `GEOGCS`, `GEOCCS`, `COMPD_CS`.
//! - WKT2: `PROJCRS`, `GEOGCRS`, `GEODCRS`, `COMPOUNDCRS`.
//!
//! The EPSG code is read from the outermost authority clauses, which are
//! `AUTHORITY["EPSG","NNNN"]` in WKT1 and `ID["EPSG",NNNN]` (code often
//! **unquoted**) in WKT2. Two different EPSG codes at that level are an
//! error. For a compound CRS the scanner descends into the one horizontal
//! subblock (a second one is an error) and reports the vertical component as stripped
//! via `WktExtraction::vertical_stripped` — the caller decides what to do
//! (mesh warns; PC warns for LAS WKT records and ignores it for sidecars). BOUNDCRS and TOWGS84 operations are rejected
//! because an EPSG code alone cannot preserve their explicit transformation.

use crate::error::CrsError;

/// Result of a successful WKT scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WktExtraction {
    pub epsg: u16,
    /// `true` iff the input was a compound CRS (`COMPD_CS[...]` or
    /// `COMPOUNDCRS[...]`) and the vertical component was discarded.
    pub vertical_stripped: bool,
}

/// Extract the EPSG code from a CRS WKT string.
///
/// Unlike [`parse_crs_string`](crate::parse_crs_string), this function
/// returns the GeoTIFF sentinel codes `0` and `32767` as `Ok`. A caller
/// that passes the code on to [`Reprojector::new`](crate::Reprojector::new)
/// must first check it with [`is_geotiff_sentinel`](crate::is_geotiff_sentinel).
///
/// # Errors
///
/// [`CrsError::Parse`] when the WKT has an unsupported root, unbalanced
/// brackets, a `(` or `)` delimiter, a `BOUNDCRS` or `TOWGS84` operation,
/// no EPSG `AUTHORITY`/`ID` clause, two different EPSG codes in one
/// block, or a compound CRS with more than one horizontal block.
pub fn extract_epsg_from_wkt(wkt: &str) -> Result<WktExtraction, CrsError> {
    reject_explicit_transforms(wkt)?;
    let bytes = wkt.as_bytes();
    let after_ws = skip_whitespace(bytes, 0);
    let (id, after_id) = read_identifier(bytes, after_ws).ok_or_else(|| {
        CrsError::parse("CRS WKT is empty or has no leading identifier".to_string())
    })?;
    let id_upper = id.to_ascii_uppercase();
    let id_shown = echo_fragment(id);
    let after_open = skip_whitespace(bytes, after_id);
    if after_open >= bytes.len() || bytes[after_open] != b'[' {
        return Err(CrsError::parse(format!(
            "CRS WKT identifier '{id_shown}' is not followed by '['"
        )));
    }
    let close_idx = find_matching_bracket(bytes, after_open).ok_or_else(|| {
        CrsError::parse(format!(
            "CRS WKT identifier '{id_shown}' has unbalanced brackets"
        ))
    })?;
    let body = &wkt[after_open + 1..close_idx];

    match id_upper.as_str() {
        // Compound CRS (WKT1 COMPD_CS / WKT2 COMPOUNDCRS): take the one
        // horizontal subblock, mark the vertical component stripped.
        "COMPD_CS" | "COMPOUNDCRS" => {
            let horizontal = match find_child_blocks(body, HORIZONTAL_KEYWORDS).as_slice() {
                [] => {
                    return Err(CrsError::parse(
                        "compound CRS contains no horizontal (PROJCS/GEOGCS/GEOCCS/\
                         PROJCRS/GEOGCRS/GEODCRS) subblock to extract EPSG from"
                            .to_string(),
                    ));
                }
                [one] => *one,
                more => {
                    return Err(CrsError::parse(format!(
                        "compound CRS contains more than one horizontal (PROJCS/GEOGCS/\
                         GEOCCS/PROJCRS/GEOGCRS/GEODCRS) subblock ({} found); the CRS \
                         identifier is ambiguous",
                        more.len()
                    )));
                }
            };
            let inner = extract_epsg_from_wkt(horizontal)?;
            Ok(WktExtraction {
                epsg: inner.epsg,
                vertical_stripped: true,
            })
        }
        // Simple CRS that directly carries the authority/ID clause.
        // WKT1: PROJCS/GEOGCS/GEOCCS. WKT2: PROJCRS/GEOGCRS/GEODCRS.
        "PROJCS" | "GEOGCS" | "GEOCCS" | "PROJCRS" | "GEOGCRS" | "GEODCRS" => {
            let epsg = parse_epsg_authority_in_body(body)?;
            Ok(WktExtraction {
                epsg,
                vertical_stripped: false,
            })
        }
        _ => Err(CrsError::parse(format!(
            "CRS WKT root is unexpected '{id_shown}' (want a WKT1 PROJCS/GEOGCS/GEOCCS/COMPD_CS \
             or WKT2 PROJCRS/GEOGCRS/GEODCRS/COMPOUNDCRS)"
        ))),
    }
}

/// Horizontal-CRS root keywords, WKT1 and WKT2. Used to find the
/// horizontal component of a compound CRS.
const HORIZONTAL_KEYWORDS: &[&str] = &[
    "PROJCS", "GEOGCS", "GEOCCS", // WKT1
    "PROJCRS", "GEOGCRS", "GEODCRS", // WKT2
];

/// Scan every nesting level without recursion and reject two kinds of
/// input outside quoted strings:
///
/// - the identifiers `BOUNDCRS` and `TOWGS84` (ASCII case-insensitive),
///   whatever character follows them, because an EPSG code alone cannot
///   carry their explicit transformation;
/// - a `(` or `)` delimiter, because the block scanner understands square
///   brackets only.
///
/// Quoted names (with `""` escapes) are exempt from both checks.
fn reject_explicit_transforms(wkt: &str) -> Result<(), CrsError> {
    let bytes = wkt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'"' {
                    if bytes.get(i + 1) == Some(&b'"') {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else if bytes[i] == b'(' || bytes[i] == b')' {
            return Err(CrsError::parse(format!(
                "CRS WKT contains '{}' outside a quoted string; this parser supports \
                 square-bracket delimiters only",
                char::from(bytes[i])
            )));
        } else if let Some((id, end)) = read_identifier(bytes, i) {
            if id.eq_ignore_ascii_case("BOUNDCRS") || id.eq_ignore_ascii_case("TOWGS84") {
                return Err(CrsError::parse(format!(
                    "{} declares an explicit datum transformation that this EPSG-only parser cannot preserve; reproject the data with that transformation before conversion",
                    id.to_ascii_uppercase()
                )));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// Longest fragment of caller input, in characters, that an error message
/// repeats back.
pub(crate) const ECHO_MAX_CHARS: usize = 64;

/// Return `fragment` for use in an error message: unchanged when it has at
/// most [`ECHO_MAX_CHARS`] characters, otherwise its first `ECHO_MAX_CHARS`
/// characters followed by `...`.
pub(crate) fn echo_fragment(fragment: &str) -> String {
    match fragment.char_indices().nth(ECHO_MAX_CHARS) {
        Some((cut, _)) => format!("{}...", &fragment[..cut]),
        None => fragment.to_string(),
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

/// Walk the depth-0 tokens of `body` (the contents of a block, excluding
/// its outer brackets) and return the substring of every child block
/// whose identifier (case-insensitive) is in `keywords`, in input order.
fn find_child_blocks<'a>(body: &'a str, keywords: &[&str]) -> Vec<&'a str> {
    let mut blocks = Vec::new();
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
            let Some((id, after_id)) = read_identifier(bytes, i) else {
                return blocks;
            };
            let after_ws = skip_whitespace(bytes, after_id);
            if after_ws < bytes.len() && bytes[after_ws] == b'[' {
                let Some(close) = find_matching_bracket(bytes, after_ws) else {
                    return blocks;
                };
                if keywords.iter().any(|k| id.eq_ignore_ascii_case(k)) {
                    blocks.push(&body[i..=close]);
                }
                // Skip past this block, so nested blocks are not counted.
                i = close + 1;
                continue;
            }
            i = after_id;
            continue;
        }
        i += 1;
    }
    blocks
}

/// Scan `body` (the contents of a PROJCS/GEOGCS/GEOCCS block, excluding
/// its outer brackets) for every `AUTHORITY[...]` / `ID[...]` clause at
/// depth 0 and return the EPSG u16 code they declare.
///
/// - Two EPSG clauses with different codes are an error (ambiguous
///   identifier). Repeats of one code are accepted.
/// - Clauses of other authorities are ignored when an EPSG clause exists;
///   with no EPSG clause, the first other authority is reported.
/// - Any clause whose arguments cannot be read, and any EPSG clause whose
///   code is not a `u16`, is an error.
fn parse_epsg_authority_in_body(body: &str) -> Result<u16, CrsError> {
    let mut epsg: Option<u16> = None;
    let mut other_authority: Option<&str> = None;
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
                // WKT1 `AUTHORITY["EPSG","NNNN"]` or WKT2 `ID["EPSG",NNNN]`.
                if id.eq_ignore_ascii_case("AUTHORITY") || id.eq_ignore_ascii_case("ID") {
                    let (authority, code_str) = split_authority_args(&body[after_ws + 1..close])?;
                    if authority.eq_ignore_ascii_case("EPSG") {
                        let code = parse_epsg_code(code_str)?;
                        if let Some(first) = epsg.filter(|&first| first != code) {
                            return Err(CrsError::parse(format!(
                                "CRS identifier is ambiguous: one block carries EPSG codes \
                                 {first} and {code} in its AUTHORITY/ID clauses"
                            )));
                        }
                        epsg = Some(code);
                    } else if other_authority.is_none() {
                        other_authority = Some(authority);
                    }
                }
                i = close + 1;
                continue;
            }
            i = after_id;
            continue;
        }
        i += 1;
    }
    match (epsg, other_authority) {
        (Some(code), _) => Ok(code),
        (None, Some(authority)) => Err(CrsError::parse(format!(
            "CRS WKT uses non-EPSG authority '{}'; only EPSG is supported",
            echo_fragment(authority)
        ))),
        (None, None) => Err(CrsError::parse(
            "CRS WKT has no EPSG AUTHORITY/ID clause (ESRI-style .prj files omit it); \
             this parser resolves a CRS by its EPSG code only, so the EPSG code must be \
             supplied separately"
                .to_string(),
        )),
    }
}

/// Split the contents of an `AUTHORITY`/`ID` block into the authority
/// name and the code text. Handles both WKT1 `"EPSG","NNNN"` (code quoted)
/// and WKT2 `"EPSG",NNNN` (code unquoted), and tolerates extra WKT2
/// trailing args (e.g. `…,VERSION[...]` / `…,URI[...]`) by reading only
/// the first two.
fn split_authority_args(inner: &str) -> Result<(&str, &str), CrsError> {
    let parts: Vec<&str> = split_outside_quotes(inner)
        .into_iter()
        .map(str::trim)
        .collect();
    if parts.len() < 2 {
        return Err(CrsError::parse(format!(
            "AUTHORITY/ID clause must have at least two arguments, got {} ({:?})",
            parts.len(),
            echo_fragment(inner)
        )));
    }
    let authority = strip_quotes(parts[0])?;
    // WKT1 quotes the code; WKT2 leaves it bare. Accept either.
    let code_str = strip_quotes_opt(parts[1]);
    Ok((authority, code_str))
}

/// Split `text` at each comma that is outside a quoted string. A quoted
/// string can hold a comma and the `""` escape.
fn split_outside_quotes(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_string = false;
    for (i, c) in text.char_indices() {
        match c {
            // A `""` escape toggles twice, which leaves the state unchanged.
            '"' => in_string = !in_string,
            ',' if !in_string => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Parse the code text of an EPSG `AUTHORITY`/`ID` clause.
fn parse_epsg_code(code_str: &str) -> Result<u16, CrsError> {
    code_str.parse::<u16>().map_err(|e| {
        CrsError::parse(format!(
            "AUTHORITY/ID code is not a valid EPSG u16: {:?}: {e}",
            echo_fragment(code_str)
        ))
    })
}

/// Strip surrounding double-quotes if present; otherwise return the
/// trimmed input unchanged (WKT2 numeric codes are unquoted).
fn strip_quotes_opt(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn strip_quotes(s: &str) -> Result<&str, CrsError> {
    let s = s.trim();
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return Err(CrsError::parse(format!(
            "AUTHORITY argument is not quoted: {:?}",
            echo_fragment(s)
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

    // ---- WKT2 (ISO 19162) ----

    #[test]
    fn wkt2_geogcrs_unquoted_id_yields_4326() {
        // WKT2 geographic: root GEOGCRS, code unquoted in ID[].
        let wkt = r#"GEOGCRS["WGS 84",
            DATUM["World Geodetic System 1984",
                ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1]]],
            CS[ellipsoidal,2],
                AXIS["geodetic latitude (Lat)",north],
                AXIS["geodetic longitude (Lon)",east],
                ANGLEUNIT["degree",0.0174532925199433],
            ID["EPSG",4326]]"#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 4326);
        assert!(!r.vertical_stripped);
    }

    #[test]
    fn wkt2_geodcrs_nad83_yields_4269() {
        let wkt = r#"GEODCRS["NAD83",
            DATUM["North American Datum 1983",
                ELLIPSOID["GRS 1980",6378137,298.257222101]],
            CS[ellipsoidal,2],AXIS["lat",north],AXIS["lon",east],
            ID["EPSG",4269]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 4269);
    }

    #[test]
    fn wkt2_projcrs_takes_projected_id_not_base() {
        // The outermost ID (32617) is the projected code; the nested
        // BASEGEOGCRS ID (4326) must NOT shadow it.
        let wkt = r#"PROJCRS["WGS 84 / UTM zone 17N",
            BASEGEOGCRS["WGS 84",
                DATUM["World Geodetic System 1984",
                    ELLIPSOID["WGS 84",6378137,298.257223563]],
                ID["EPSG",4326]],
            CONVERSION["UTM zone 17N",METHOD["Transverse Mercator",ID["EPSG",9807]]],
            CS[Cartesian,2],AXIS["(E)",east],AXIS["(N)",north],
            LENGTHUNIT["metre",1],
            ID["EPSG",32617]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
    }

    #[test]
    fn wkt2_compoundcrs_strips_vertical() {
        let wkt = r#"COMPOUNDCRS["NAD83 / UTM 17N + NAVD88",
            PROJCRS["NAD83 / UTM zone 17N",
                BASEGEOGCRS["NAD83",DATUM["NAD83",ELLIPSOID["GRS 1980",6378137,298.257222101]],ID["EPSG",4269]],
                CONVERSION["x",METHOD["Transverse Mercator"]],
                CS[Cartesian,2],AXIS["e",east],AXIS["n",north],ID["EPSG",26917]],
            VERTCRS["NAVD88",VDATUM["North American Vertical Datum 1988"],
                CS[vertical,1],AXIS["up",up],ID["EPSG",5703]]]"#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 26917);
        assert!(r.vertical_stripped);
    }

    #[test]
    fn wkt2_boundcrs_is_rejected() {
        let wkt = r#"BOUNDCRS[
            SOURCECRS[GEOGCRS["unknown",DATUM["d",ELLIPSOID["GRS 1980",6378137,298.257222101]],
                CS[ellipsoidal,2],AXIS["lat",north],AXIS["lon",east],ID["EPSG",4269]]],
            TARGETCRS[GEOGCRS["WGS 84",DATUM["WGS84",ELLIPSOID["WGS 84",6378137,298.257223563]],ID["EPSG",4326]]],
            ABRIDGEDTRANSFORMATION["NAD83 to WGS84",METHOD["Geocentric translations"],
                PARAMETER["X",0,LENGTHUNIT["metre",1]]]]"#;
        assert!(
            extract_epsg_from_wkt(wkt)
                .unwrap_err()
                .to_string()
                .contains("BOUNDCRS")
        );
    }

    #[test]
    fn wkt2_id_with_trailing_uri_arg_is_tolerated() {
        let wkt = r#"GEOGCRS["WGS 84",DATUM["d",ELLIPSOID["WGS 84",6378137,298.257223563]],
            CS[ellipsoidal,2],AXIS["lat",north],AXIS["lon",east],
            ID["EPSG",4326,URI["http://www.opengis.net/def/crs/EPSG/0/4326"]]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 4326);
    }

    #[test]
    fn two_different_epsg_codes_in_one_block_are_ambiguous() {
        for wkt in [
            r#"PROJCS["x",AUTHORITY["EPSG","32617"],AUTHORITY["EPSG","32616"]]"#,
            r#"PROJCRS["x",ID["EPSG",32617],ID["EPSG",32616]]"#,
            r#"PROJCRS["x",ID["EPSG",32617],AUTHORITY["EPSG","32616"]]"#,
        ] {
            let msg = extract_epsg_from_wkt(wkt).unwrap_err().to_string();
            assert!(msg.contains("ambiguous"), "msg: {msg}");
            assert!(msg.contains("32617") && msg.contains("32616"), "msg: {msg}");
        }
    }

    #[test]
    fn repeated_identical_epsg_code_is_accepted() {
        let wkt = r#"PROJCRS["x",ID["EPSG",32617],AUTHORITY["EPSG","32617"]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
    }

    #[test]
    fn other_authorities_are_ignored_when_an_epsg_clause_exists() {
        for wkt in [
            r#"PROJCS["x",AUTHORITY["ESRI","102100"],AUTHORITY["EPSG","3857"]]"#,
            r#"PROJCRS["x",ID["EPSG",3857],ID["ESRI",102100]]"#,
        ] {
            assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 3857, "{wkt}");
        }
    }

    #[test]
    fn comma_inside_a_quoted_authority_name_does_not_split_the_clause() {
        let wkt = r#"PROJCRS["x",ID["EPSG",32617],ID["Some, Org",7]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
        let wkt = r#"PROJCS["x",AUTHORITY["Some ""quoted"", Org","7"],AUTHORITY["EPSG","32617"]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
    }

    #[test]
    fn malformed_epsg_clause_after_a_valid_one_is_an_error() {
        let wkt = r#"PROJCS["x",AUTHORITY["EPSG","32617"],AUTHORITY["EPSG","zone"]]"#;
        let msg = extract_epsg_from_wkt(wkt).unwrap_err().to_string();
        assert!(msg.contains("not a valid EPSG u16"), "msg: {msg}");
    }

    #[test]
    fn nested_authority_clauses_do_not_count_as_ambiguous() {
        let wkt = r#"PROJCS["x",GEOGCS["g",AUTHORITY["EPSG","4326"]],
            PROJECTION["tm",AUTHORITY["EPSG","9807"]],AUTHORITY["EPSG","32617"]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
    }

    #[test]
    fn compound_with_two_horizontal_blocks_is_an_error() {
        for wkt in [
            r#"COMPD_CS["c",PROJCS["a",AUTHORITY["EPSG","32616"]],
                PROJCS["b",AUTHORITY["EPSG","32617"]],VERT_CS["v",AUTHORITY["EPSG","5703"]]]"#,
            r#"COMPOUNDCRS["c",PROJCRS["a",ID["EPSG",32616]],
                GEOGCRS["b",ID["EPSG",4326]]]"#,
        ] {
            let msg = extract_epsg_from_wkt(wkt).unwrap_err().to_string();
            assert!(msg.contains("more than one horizontal"), "msg: {msg}");
        }
    }

    /// The scanner reads LAS records, sidecar files and E57 metadata, so its
    /// messages name neither a container format nor a conversion tool.
    #[test]
    fn error_messages_are_context_neutral() {
        for wkt in [
            "",
            "PROJCS",
            "PROJCS[",
            r#"VERT_CS["v",AUTHORITY["EPSG","5773"]]"#,
            r#"PROJCS["x",UNIT["m",1]]"#,
            r#"PROJCS["x",AUTHORITY["ESRI","102100"]]"#,
        ] {
            let msg = extract_epsg_from_wkt(wkt).unwrap_err().to_string();
            assert!(msg.contains("CRS WKT"), "{wkt:?}: {msg}");
            assert!(!msg.contains("VLR"), "{wkt:?}: {msg}");
            assert!(!msg.contains("pdal"), "{wkt:?}: {msg}");
        }
    }

    #[test]
    fn missing_identifier_message_explains_the_esri_case() {
        let msg = extract_epsg_from_wkt(r#"PROJCS["x",UNIT["m",1]]"#)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("no EPSG AUTHORITY/ID clause"), "msg: {msg}");
        assert!(msg.contains("ESRI-style .prj"), "msg: {msg}");
        assert!(msg.contains("EPSG code only"), "msg: {msg}");
    }

    /// Every error that echoes caller input stays short and still shows the
    /// start of the echoed fragment followed by the truncation marker.
    #[test]
    fn long_input_fragments_are_truncated_in_errors() {
        let long = "A".repeat(65_536);
        let shown = format!("{}...", "A".repeat(ECHO_MAX_CHARS));
        let cases = [
            long.clone(),
            format!("{long}["),
            format!("{long}[]"),
            format!(r#"PROJCS["x",AUTHORITY[{long}]]"#),
            format!(r#"PROJCS["x",AUTHORITY["{long}","1"]]"#),
            format!(r#"PROJCS["x",AUTHORITY["EPSG","{long}"]]"#),
            format!(r#"PROJCS["x",AUTHORITY[{long},"1"]]"#),
        ];
        for wkt in &cases {
            let msg = extract_epsg_from_wkt(wkt).unwrap_err().to_string();
            assert!(msg.len() < 400, "len {}: {}", msg.len(), &msg[..200]);
            assert!(msg.contains(&shown), "msg: {msg}");
        }
    }

    #[test]
    fn echo_fragment_keeps_short_text_and_cuts_on_a_char_boundary() {
        assert_eq!(echo_fragment("EPSG"), "EPSG");
        let exact = "é".repeat(ECHO_MAX_CHARS);
        assert_eq!(echo_fragment(&exact), exact);
        let over = "é".repeat(ECHO_MAX_CHARS + 1);
        assert_eq!(echo_fragment(&over), format!("{exact}..."));
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
