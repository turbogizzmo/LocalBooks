# Vendored crates

## sqlx-sqlite 0.9.0

The published `sqlx-sqlite` 0.9.0 crate (crates.io checksum
`488e99c397a62007e4229aec669a179816339afc6d2620ca6fa420dbee2e982c`), unpacked as released, with
one change: its `libsqlite3-sys` range in `Cargo.toml` is `>=0.30.1, <0.41.0` instead of
`>=0.30.1, <0.38.0`. The files Cargo adds when it packages a crate (`Cargo.lock`,
`Cargo.toml.orig`, `.cargo_vcs_info.json`) are left out. Its licenses (MIT or Apache-2.0) are
`LICENSE-MIT` and `LICENSE-APACHE` here.

**Why.** RUSTSEC-2026-0459 (gap R24 in `docs/gaps.md`): SQLite before 3.54 can return
uninitialized memory when a database is opened with a `-wal` file of another page size. Tillhouse
builds SQLCipher through `libsqlite3-sys`'s `bundled-sqlcipher` feature, and the first release
whose SQLCipher sits on a fixed SQLite is `libsqlite3-sys` 0.40.0 (SQLCipher 4.20.0, SQLite
3.54.1; 0.39's SQLCipher is still SQLite 3.53.4). No released `sqlx-sqlite` accepts 0.40 yet.
Upstream raises the top of this range in patch releases without code changes (launchbadge/sqlx
pull request 4502 raised it to `<0.40.0` on `main`, a one-line `Cargo.toml` change), and
`sqlx-sqlite` 0.9.0 compiles and passes the full test suite against 0.40.0.

`src-tauri/Cargo.toml` applies it with `[patch.crates-io]`.

**Removing it.** When a released `sqlx-sqlite` accepts `libsqlite3-sys` 0.40, delete this folder
and the `[patch.crates-io]` section, bump `sqlx` to that release, and run `cargo update -p sqlx`.

**Checking it.** Unpack the published crate and compare:

```
curl -sL https://static.crates.io/crates/sqlx-sqlite/sqlx-sqlite-0.9.0.crate | tar xz -C /tmp
diff -r /tmp/sqlx-sqlite-0.9.0 src-tauri/vendor/sqlx-sqlite
```

The only differences are the three packaging files above and the version range.
