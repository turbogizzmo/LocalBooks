# Third-party notice supplements

`scripts/third-party-notices.mjs` builds `THIRD-PARTY-NOTICES.txt` from the license and NOTICE
files that each shipped crate and npm package publishes. This folder fills the gaps:

- `extra/<package>/`: files a published package leaves out but its project carries. Used only when
  the package has no file of that kind (license or NOTICE) of its own.
  - `opensqlany/NOTICE`: the crate on crates.io (0.2.0) has no NOTICE; this is the NOTICE from
    github.com/Sigilweaver/OpenSQLAnywhere at tag `v0.2.0`. Remove it once a release ships one.
- `licenses/<SPDX id>.txt`: standard license texts from SPDX license-list-data v3.27.0, printed for
  packages that ship no license file at all. Add a text when the generator lists a package whose
  license has none here.
