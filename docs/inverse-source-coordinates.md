# Inverse source coordinates

Date: 2026-10-04. `Reprojector::from_ecef` recovers coordinates in the
selected source CRS from Earth-centered metres. Mesh needs this operation
to evaluate a source normal at its centered output vertex without retaining
another source-position table.

Geographic output uses longitude and latitude in degrees, in that order.
Height retains the source vertical unit. Projected output retains separate
horizontal and vertical units. EPSG:2926 uses horizontal survey feet and
vertical metres. Identity EPSG:4978 preserves finite coordinate bits.
Nonfinite input or output fails with `CrsError::Reproject`.

Native validation passes 80 tests each in debug, release, and all-feature
runs. Formatting and strict all-target, all-feature Clippy pass. The five
inverse tests include closed-form WGS84 coordinates, the frozen Kingston
PROJ control, independent survey-foot controls, identity bits, and errors.
The existing forward and parity tests also pass.

The first native test attempt stops on an ignored lockfile that still names
package version 0.1.0. The corrected lockfile names 0.3.0 and retains the
same registry dependency versions. The second baseline stops because the
inverse method does not exist. This is an absent-API baseline, not a
numerical failure. The implementation then passes the complete suite.

Evidence: `/mnt/data2/tileforge-completion-20261004/crs-inverse-green-v1/`.
Archive SHA256:
`f0cbd123af4264d760f1431f06255a9aaee0e5faaa2b5d759f6c240c92e9294c`.
The archive retains the resolved lockfile. The repository ignores its
library lockfile, so the native archive supplies dependency provenance.
Mesh integration and coordinated release validation remain separate gates.
