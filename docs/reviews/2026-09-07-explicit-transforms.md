# Explicit WKT transformations

> **Status:** Historical validation record. The rejection ships in 0.3.0, and both consumer pins (mesh, pc) include it as of 2026-10-05. CRS-01 is the finding ID from the 2026-09-06 family review, whose raw output is not kept in Git. The evidence directory on `.212` is outside Git and was not re-checked for this status line.
> **Summary:** Why the parser rejects WKT `BOUNDCRS` and `TOWGS84` blocks instead of discarding them, and how the change was validated.

CRS-01. Base revision: d38eb25d9b5b64f5cfa17364cf35a82488fccaec. The synthetic bound-shift fixture comes from the September 6 review's PROJ oracle. Discarding its declared 100, 200, and 300 metre translations changed the result by 374.1656329841366 metres.

The new regression confirms that BOUNDCRS and TOWGS84 previously returned an EPSG code without preserving the operation. The parser now rejects those blocks, at any nesting level, with a specific Parse error. Quoted names and escaped quotes do not trigger rejection. Zero-valued TOWGS84 operations are also rejected; the API does not establish equivalence to the operation associated with an authority code.

The public API still represents a CRS by EPSG code. No full transformation model is added. Callers must reproject such data with its declared operation before conversion. Merely deleting the WKT operation is not a correction.

All tests and compilation run on 192.168.8.212 under /mnt/data2/crs/review-fixes-20260907 with Rust 1.94.1 and CARGO_BUILD_JOBS=4:

```sh
cargo test
cargo test --release
cargo clippy --all-targets -- -D warnings
```

All final commands exit 0: 56 unit tests, four explicit-transform regressions, two independent grid-oracle tests, and 11 other integration tests pass in both profiles. An intermediate test edit accidentally changed the ordinary NAD83 expectation; that edit was corrected before final validation. Production behavior for ordinary NAD83 remains covered.

Raw evidence stays outside Git on .212 in /mnt/data2/crs/review-fixes-20260907/evidence. The red.log, green-final.log, release-final.log, and clippy-final.log files record the runs. The small input fixture is committed under tests/fixtures/bound-shift.wkt.

When this record was written, the change was pull request #1 and the consumer pins did not include it. Both pins moved later (see the status line). This record does not prove consumer-side error propagation.
