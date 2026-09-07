use tileforge_crs::{extract_epsg_from_wkt, parse_crs_string};

#[test]
fn bound_transform_is_not_silently_discarded() {
    let wkt = include_str!("fixtures/bound-shift.wkt");
    let err = extract_epsg_from_wkt(wkt).unwrap_err();
    assert!(err.to_string().contains("BOUNDCRS"));
    assert!(parse_crs_string(wkt).is_err());
}

#[test]
fn towgs84_is_not_silently_discarded() {
    for operation in ["TOWGS84[100,200,300]", "towgs84 [0,0,0,0,0,0,0]"] {
        let wkt =
            format!("GEOGCS[\"custom\",DATUM[\"datum\",{operation}],AUTHORITY[\"EPSG\",\"4269\"]]");
        let err = extract_epsg_from_wkt(&wkt).unwrap_err();
        assert!(err.to_string().contains("TOWGS84"));
    }
}

#[test]
fn quoted_operation_names_are_not_operations() {
    let wkt = r#"GEOGCS["BOUNDCRS and ""TOWGS84[1,2,3]""",AUTHORITY["EPSG","4326"]]"#;
    assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 4326);
}

#[test]
fn compound_crs_cannot_hide_a_bound_transform() {
    let wkt = format!(
        "COMPOUNDCRS[\"compound\",{},VERTCRS[\"height\"]]",
        include_str!("fixtures/bound-shift.wkt").trim()
    );
    let err = extract_epsg_from_wkt(&wkt).unwrap_err();
    assert!(err.to_string().contains("BOUNDCRS"));
}
