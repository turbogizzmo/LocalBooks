//! Page-store decoder for company files whose rows openqbw cannot find.
//!
//! openqbw removes the additive per-sector cipher but leaves each 512-byte sector circularly
//! rotated, so rows are not page-linear and its anchors miss them on newer files. This module
//! finishes the job, following the structure described in OpenSQLAnywhere issue #16 and checked
//! byte by byte on a real file:
//!
//! 1. Sectors 0..6: the cipher is `base + s` plus `i * step_s` per sector. The decoded sector 0
//!    contains the page's own number exactly once, at an odd offset `r0`; sector `s` is rotated
//!    by `r0 - 2 * floor((s + q) / 2)`, where the pairing phase `q` is 0 or 1 per page.
//! 2. Sector 7 (496 bytes plus the plaintext trailer) carries an extra term `d * C[r7](i)`: `d`
//!    is an odd per-page multiplier and `C` a table of values 0..=16 that depends only on the
//!    sector's rotation `r7`. Rows of `C` are learned from pages whose sector 7 is all zero, then
//!    each page's `(step, d)` is solved against the row lengths already known from its directory.
//!    After rotation, the 16 trailer bytes sit inside the sector and the 16 data bytes they
//!    displaced are at its end.
//! 3. The last 12 bytes of the row area are stashed in the page header (bytes 18..20 and 6..16).
//!
//! The result is the logical page: header (u32 page number, kind at 17, lowest row offset at 20,
//! slot count at 22, owning table id at 24), slot directory at 28 (offsets relative to +28), and
//! rows that each start with their own u16 length and end at 4092.

use std::collections::HashMap;

use openqbw::{deobfuscate_with_bv, recover_bv_any};
use opensqlany::{ApModel, PageStore, PageType};

pub const PAGE_SIZE: usize = 4096;
/// Logical bytes recovered per page: the row area ends here.
pub const ROW_AREA_END: usize = 4092;
const SECTOR: usize = 512;
/// Logical offset where sector 7 begins.
pub const S7_START: usize = 7 * SECTOR;
const S7_LEN: usize = 496;
const SLOT_BASE: usize = 28;
const MAX_SLOTS: usize = 1500;

/// Values of the sector-7 correction table never exceed this.
const C_MAX: u8 = 16;

/// One row as the slot directory describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    pub start: u16,
    pub len: u16,
    /// The row's own u16 length matches the gap to the next row.
    pub chained: bool,
}

/// How sector 7 of a page was recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sector7 {
    /// Solved with an exact correction row and every row length in the sector agreeing.
    Verified,
    /// Solved, but with an estimated correction row or without every row length agreeing.
    BestEffort,
    /// Could not be solved; logical bytes 3584..4080 are unreliable.
    Unsolved,
}

#[derive(Debug, Clone)]
pub struct LogicalPage {
    pub number: u64,
    pub kind: u8,
    pub owner: u32,
    pub bytes: Vec<u8>,
    pub rows: Vec<RowSpan>,
    pub sector7: Sector7,
    /// Diagnostics: pairing phase, sector-7 rotation, whether its correction row is exact.
    #[cfg_attr(not(test), allow(dead_code))]
    pub q: u8,
    pub r7: u16,
    #[cfg_attr(not(test), allow(dead_code))]
    pub c_exact: Option<bool>,
}

impl LogicalPage {
    /// The bytes of a row, when its length chains.
    pub fn row(&self, span: &RowSpan) -> &[u8] {
        &self.bytes[span.start as usize..(span.start + span.len) as usize]
    }
}

/// What the decoder learned about a page before sector 7 is solved.
struct Prepared {
    number: u64,
    raw: Vec<u8>,
    /// Sector 7's cipher base: the page base plus 7, or sector 6's repaired base plus 1.
    base7: u8,
    steps: [u8; 7],
    r0: usize,
    q: usize,
    /// Rotation used for each of sectors 0..6, after repair.
    rots: [usize; 7],
    /// Logical bytes 0..3584 (sectors 0..6, un-rotated).
    head: Vec<u8>,
}

impl Prepared {
    fn rot(&self, s: usize) -> usize {
        (self.r0 + 2 * SECTOR - 2 * ((s + self.q) / 2)) % SECTOR
    }

    fn r7(&self) -> usize {
        let rot6 = self.rots[6];
        if self.q == 0 { rot6 } else { (rot6 + SECTOR - 2) % SECTOR }
    }

    fn slot_starts(&self) -> Vec<usize> {
        // A wrong anchor candidate can give any slot count; read only what fits.
        let n = (u16le(&self.head, 22) as usize).min(MAX_SLOTS).min((self.head.len() - SLOT_BASE) / 2);
        let mut starts: Vec<usize> = (0..n)
            .map(|i| u16le(&self.head, SLOT_BASE + 2 * i) as usize)
            .filter(|&v| v != 0)
            .map(|v| SLOT_BASE + v)
            .collect();
        starts.sort_unstable();
        starts.dedup();
        starts
    }

    /// Sector-7 bytes with the linear cipher removed for a candidate step: `plain + d * C[i]`.
    fn residual(&self, step7: u8) -> [u8; S7_LEN] {
        let base7 = self.base7;
        let mut out = [0u8; S7_LEN];
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.raw[S7_START + i].wrapping_sub(base7).wrapping_sub((i as u8).wrapping_mul(step7));
        }
        out
    }

    fn step7_candidates(&self) -> Vec<u8> {
        if self.q == 0 { vec![self.steps[6]] } else { (0..=255).collect() }
    }
}

fn u16le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn inverse_mod_256(d: u8) -> u8 {
    // Newton iteration for the inverse of an odd number modulo 2^8.
    let mut x = d;
    for _ in 0..3 {
        x = x.wrapping_mul(2u8.wrapping_sub(d.wrapping_mul(x)));
    }
    x
}

/// Stored sector-7 byte that logical sector-7 offset `x` (0..496) comes from. In the rotated
/// 512-byte unit (496 data bytes then the 16-byte trailer) the trailer occupies 16 slots; the 16
/// data bytes it displaced were moved to the end of the unit and belong in those slots.
fn s7_source(x: usize, r7: usize) -> usize {
    let t = (S7_LEN + SECTOR - r7) % SECTOR;
    let j = if x < t { x } else if x < t + 16 { S7_LEN + (x - t) } else { x };
    (j + r7) % SECTOR
}

/// Trailer window position in the rotated sector; above 480 the window wraps and the page is
/// left unsolved (rare).
fn s7_solvable(r7: usize) -> bool {
    (S7_LEN + SECTOR - r7) % SECTOR <= 480
}

/// Builds the logical sector 7 from a decoded (cipher-free) stored sector.
fn s7_logical(plain7: &[u8; S7_LEN], r7: usize) -> [u8; S7_LEN] {
    let mut out = [0u8; S7_LEN];
    for (x, o) in out.iter_mut().enumerate() {
        *o = plain7[s7_source(x, r7)];
    }
    out
}

/// Scores how plausible decoded row bytes are: zeros and ASCII digits are common, high bytes
/// rarer. Wrong solutions inject 0x40/0x80/0xC0 noise.
fn plausibility(bytes: &[u8]) -> i32 {
    bytes
        .iter()
        .map(|&b| match b {
            0x00 | 0x30 => 2,
            0x20..=0x7E | b'\t' | b'\n' | b'\r' => 1,
            0x80..=0xFF => -1,
            _ => 0,
        })
        .sum()
}

/// Reads and decodes every row page of a company file.
pub struct PageDecoder {
    store: PageStore,
    model: ApModel,
    /// Learned sector-7 correction rows by rotation, and whether each is exact.
    c_rows: HashMap<usize, (Vec<u8>, bool)>,
}

/// Counts from a decode run, for diagnostics and the importer's summary.
#[derive(Debug, Default, Clone)]
pub struct DecodeStats {
    pub row_pages: u64,
    pub decoded_pages: u64,
    pub no_anchor_pages: u64,
    pub bad_header_pages: u64,
    pub exact_c_rows: u64,
    pub estimated_c_rows: u64,
    pub s7_verified: u64,
    pub s7_best_effort: u64,
    pub s7_unsolved: u64,
    pub rows: u64,
    pub chained_rows: u64,
    pub timings: Vec<(&'static str, f32)>,
}

impl PageDecoder {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let store = PageStore::open(path).map_err(|e| format!("QBW_READ: could not read {}: {e}", path.display()))?;
        let model = ApModel::learn(&store);
        Ok(Self { store, model, c_rows: HashMap::new() })
    }


    fn row_page_numbers(&self) -> Vec<u64> {
        (1..self.store.page_count())
            .filter(|&pn| {
                self.store.page(pn).map(|p| p.trailer().page_type() == PageType::Extent).unwrap_or(false)
            })
            .collect()
    }

    /// Removes the cipher from sectors 0..6, finds the rotation anchor and pairing phase.
    fn prepare(&self, pn: u64) -> Option<Prepared> {
        let page = self.store.page(pn).ok()?;
        let raw = page.bytes().to_vec();
        let dec = match recover_bv_any(pn, &raw) {
            Some(bv) => deobfuscate_with_bv(&raw, pn, bv),
            None => self.model.deobfuscate_with_store(&raw, pn, &self.store),
        };
        let base = raw[0].wrapping_sub(dec[0]);
        let mut steps = [0u8; 7];
        for (s, step) in steps.iter_mut().enumerate() {
            let k0 = raw[s * SECTOR].wrapping_sub(dec[s * SECTOR]);
            let k1 = raw[s * SECTOR + 1].wrapping_sub(dec[s * SECTOR + 1]);
            *step = k1.wrapping_sub(k0);
        }

        // The page number anchors sector 0's rotation. A row can hold the same four bytes (a row
        // id equal to the page number), so every odd position is a candidate; the one whose
        // header validates and whose rows chain best wins below.
        let anchor = (pn as u32).to_le_bytes();
        let find_anchors = |dec: &[u8]| -> Vec<usize> {
            (1..SECTOR).step_by(2).filter(|&i| (0..4).all(|k| dec[(i + k) % SECTOR] == anchor[k])).collect()
        };
        let model_anchors = find_anchors(&dec);
        let (dec, base, steps, anchors) = if model_anchors.is_empty() {
            let (base, steps, dec, r0) = recover_cipher(pn, &raw)?;
            (dec, base, steps, vec![r0])
        } else {
            (dec, base, steps, model_anchors)
        };

        // Sectors in a pair share their step, so the step pattern usually gives the phase.
        let pairs0 = [(0, 1), (2, 3), (4, 5)].iter().filter(|(a, b)| steps[*a] == steps[*b]).count();
        let pairs1 = [(1, 2), (3, 4), (5, 6)].iter().filter(|(a, b)| steps[*a] == steps[*b]).count();
        let phases: &[usize] = match pairs0.cmp(&pairs1) {
            std::cmp::Ordering::Greater => &[0],
            std::cmp::Ordering::Less => &[1],
            std::cmp::Ordering::Equal => &[0, 1],
        };

        let mut best: Option<(i64, Prepared)> = None;
        for (&r0, &q) in anchors.iter().flat_map(|r| phases.iter().map(move |q| (r, q))) {
            let mut p = Prepared { number: pn, raw: raw.clone(), base7: base.wrapping_add(7), steps, r0, q, rots: [0; 7], head: Vec::with_capacity(S7_START) };
            for s in 0..7 {
                p.rots[s] = p.rot(s);
                let sec = &dec[s * SECTOR..(s + 1) * SECTOR];
                p.head.extend_from_slice(&sec[p.rots[s]..]);
                p.head.extend_from_slice(&sec[..p.rots[s]]);
            }
            repair_rotations(&mut p, &dec);
            let score = head_score(&p);
            if best.as_ref().is_none_or(|(b, _)| score > *b) {
                best = Some((score, p));
            }
        }
        best.and_then(|(score, p)| (score >= 0).then_some(p))
    }

    /// Learns exact correction rows from pages whose sector 7 decodes to all zeros.
    fn learn_exact_rows(&mut self, prepared: &[Prepared]) {
        for p in prepared {
            let r7 = p.r7();
            if self.c_rows.get(&r7).is_some_and(|(_, exact)| *exact) || !s7_solvable(r7) {
                continue;
            }
            'steps: for step7 in p.step7_candidates() {
                let res = p.residual(step7);
                for d in (1..=255u8).step_by(2) {
                    let inv = inverse_mod_256(d);
                    if res.iter().all(|&x| x.wrapping_mul(inv) <= C_MAX) {
                        let row: Vec<u8> = res.iter().map(|&x| x.wrapping_mul(inv)).collect();
                        // An all-zero row would mean no correction; accept it only once seen.
                        self.c_rows.insert(r7, (row, true));
                        break 'steps;
                    }
                }
            }
        }
    }

    /// Estimates rows for rotations without an all-zero page. Each round solves every page of
    /// the rotation with the current row (its step and multiplier are pinned by the row lengths
    /// it already knows), then sets each position to the correction value that turns that byte
    /// into zero on the most pages; zero is by far the most common byte in rows.
    fn learn_estimated_rows(&mut self, prepared: &[Prepared]) {
        let mut groups: HashMap<usize, Vec<&Prepared>> = HashMap::new();
        for p in prepared {
            let r7 = p.r7();
            if !self.c_rows.contains_key(&r7) && s7_solvable(r7) {
                groups.entry(r7).or_default().push(p);
            }
        }
        for (r7, pages) in groups {
            let mut row = initial_row(&pages);
            for _ in 0..4 {
                let mut votes = vec![[0u16; (C_MAX + 1) as usize]; S7_LEN];
                for p in &pages {
                    let Some(sol) = solve_s7(p, r7, &row) else { continue };
                    let res = p.residual(sol.step7);
                    for (i, v) in votes.iter_mut().enumerate() {
                        for c in 0..=C_MAX {
                            if res[i] == sol.d.wrapping_mul(c) {
                                v[c as usize] += 1;
                            }
                        }
                    }
                }
                for (i, v) in votes.iter().enumerate() {
                    if let Some((c, n)) = v.iter().enumerate().max_by_key(|(_, n)| **n) {
                        if *n > 0 {
                            row[i] = c as u8;
                        }
                    }
                }
            }
            self.c_rows.insert(r7, (row, false));
        }
    }

    /// Solves sector 7 of one page and assembles the logical page. The predicted rotation is
    /// tried first; rotations off by 2, 4 or half a sector occur on some pages and are tried too.
    fn finish(&self, p: &Prepared) -> LogicalPage {
        let starts = p.slot_starts();
        let r7 = p.r7();
        let mut bytes = p.head.clone();
        bytes.resize(ROW_AREA_END, 0);
        bytes[S7_START + S7_LEN..ROW_AREA_END].copy_from_slice(&[&p.head[18..20], &p.head[6..16]].concat());

        let mut best: Option<(i32, S7Solution, usize, bool)> = None;
        for (delta, bonus) in [(0, 50), (SECTOR - 2, 0), (2, 0), (SECTOR - 4, 0), (4, 0), (256, 0)] {
            let rv = (r7 + delta) % SECTOR;
            let Some((c_row, exact)) = self.c_rows.get(&rv) else { continue };
            if !s7_solvable(rv) {
                continue;
            }
            if let Some(sol) = solve_s7(p, rv, c_row) {
                let score = sol.score + bonus;
                if best.as_ref().is_none_or(|(b, ..)| score > *b) {
                    best = Some((score, sol, rv, *exact));
                }
            }
        }
        let (sector7, r7_used, c_exact) = match best {
            Some((_, sol, rv, exact)) => {
                bytes[S7_START..S7_START + S7_LEN].copy_from_slice(&sol.logical);
                let verified = exact && sol.known > 0 && sol.ok == sol.known;
                (if verified { Sector7::Verified } else { Sector7::BestEffort }, rv, Some(exact))
            }
            None => (Sector7::Unsolved, r7, None),
        };

        let mut rows = Vec::with_capacity(starts.len());
        for (k, &st) in starts.iter().enumerate() {
            let next = starts.get(k + 1).copied().unwrap_or(ROW_AREA_END);
            if next <= st || next > ROW_AREA_END {
                continue;
            }
            let len = (next - st) as u16;
            rows.push(RowSpan { start: st as u16, len, chained: u16le(&bytes, st) == len });
        }
        LogicalPage {
            number: p.number,
            kind: bytes[17],
            owner: u32le(&bytes, 24),
            bytes,
            rows,
            sector7,
            q: p.q as u8,
            r7: r7_used as u16,
            c_exact,
        }
    }

    /// Re-solves sector 7 of `page` when its rows fail a check the caller can make (the typed
    /// row walk). Three things can be off on such a page, so all are searched together:
    /// - rows learned from all-zero pages can be filed under the wrong rotation (when that page's
    ///   rotation had flipped), so every learned row is tried;
    /// - the rotation itself drifts by 2, 4 or half a sector, so those variants are tried;
    /// - on pages whose rotation flipped, sector 7's base differs from the one the other sectors
    ///   imply, so the base is derived from the first known row-length byte instead of assumed.
    /// Every candidate must reproduce all the sector's known row-length bytes. The one whose rows
    /// `score` rates highest wins (ties: the assumed base, then the most plausible bytes), and is
    /// returned when it beats the current decode.
    pub fn refine(&self, page: &LogicalPage, score: &dyn Fn(&LogicalPage) -> usize) -> Option<LogicalPage> {
        let p = self.prepare(page.number)?;
        let r7 = p.r7();
        let raw7 = &p.raw[S7_START..S7_START + S7_LEN];
        let current = score(page);
        let mut best: Option<((usize, bool, i32), LogicalPage)> = None;
        let mut seen: std::collections::HashSet<[u8; S7_LEN]> = std::collections::HashSet::new();
        for delta in [0, SECTOR - 2, 2, SECTOR - 4, 4, 256, 254, 258] {
            let rv = (r7 + delta) % SECTOR;
            if !s7_solvable(rv) {
                continue;
            }
            let known = s7_known(&p, rv);
            let Some(&(i0, b0)) = known.first() else { continue };
            for step7 in p.step7_candidates() {
                for (c_row, _) in self.c_rows.values() {
                    for d in (1..=255u8).step_by(2) {
                        let base7 = raw7[i0].wrapping_sub((i0 as u8).wrapping_mul(step7)).wrapping_sub(d.wrapping_mul(c_row[i0])).wrapping_sub(b0);
                        let byte = |i: usize| {
                            raw7[i].wrapping_sub(base7).wrapping_sub((i as u8).wrapping_mul(step7)).wrapping_sub(d.wrapping_mul(c_row[i]))
                        };
                        if !known.iter().all(|(i, b)| byte(*i) == *b) {
                            continue;
                        }
                        let mut plain = [0u8; S7_LEN];
                        for (i, b) in plain.iter_mut().enumerate() {
                            *b = byte(i);
                        }
                        let logical = s7_logical(&plain, rv);
                        if !seen.insert(logical) {
                            continue;
                        }
                        let mut candidate = page.clone();
                        candidate.bytes[S7_START..S7_START + S7_LEN].copy_from_slice(&logical);
                        for r in candidate.rows.iter_mut() {
                            r.chained = u16le(&candidate.bytes, r.start as usize) == r.len;
                        }
                        let rank = (score(&candidate), base7 == p.base7, plausibility(&logical));
                        if rank.0 > current && best.as_ref().is_none_or(|(b, _)| rank > *b) {
                            candidate.r7 = rv as u16;
                            best = Some((rank, candidate));
                        }
                    }
                }
            }
        }
        best.map(|(_, page)| page)
    }

    /// Decodes every row page. Blocking and CPU-bound.
    pub fn decode_all(&mut self) -> (Vec<LogicalPage>, DecodeStats) {
        let mut stats = DecodeStats::default();
        let mut prepared = Vec::new();
        let t = std::time::Instant::now();
        let numbers = self.row_page_numbers();
        stats.row_pages = numbers.len() as u64;
        for p in parallel_map(&numbers, |&pn| self.prepare(pn)) {
            match p {
                Some(p) => prepared.push(p),
                None => stats.no_anchor_pages += 1,
            }
        }
        stats.timings.push(("prepare", t.elapsed().as_secs_f32()));
        let t = std::time::Instant::now();
        self.learn_exact_rows(&prepared);
        stats.exact_c_rows = self.c_rows.len() as u64;
        stats.timings.push(("learn exact", t.elapsed().as_secs_f32()));
        let t = std::time::Instant::now();
        self.learn_estimated_rows(&prepared);
        stats.estimated_c_rows = self.c_rows.len() as u64 - stats.exact_c_rows;
        stats.timings.push(("learn estimated", t.elapsed().as_secs_f32()));
        let t = std::time::Instant::now();

        let mut pages = Vec::with_capacity(prepared.len());
        for page in parallel_map(&prepared, |p| self.finish(p)) {
            if page.bytes[16] != 4 {
                stats.bad_header_pages += 1;
                continue;
            }
            stats.decoded_pages += 1;
            match page.sector7 {
                Sector7::Verified => stats.s7_verified += 1,
                Sector7::BestEffort => stats.s7_best_effort += 1,
                Sector7::Unsolved => stats.s7_unsolved += 1,
            }
            stats.rows += page.rows.len() as u64;
            stats.chained_rows += page.rows.iter().filter(|r| r.chained).count() as u64;
            pages.push(page);
        }
        stats.timings.push(("finish", t.elapsed().as_secs_f32()));
        (pages, stats)
    }
}

/// Maps `f` over `items` on all cores, keeping order. Page work is independent and CPU-bound
/// but uneven, so workers take the next item as they finish.
fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, R)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        out.push((i, f(item)));
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("page worker panicked")).collect()
    });
    done.sort_unstable_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

/// A sector-7 solution for one page and rotation.
struct S7Solution {
    score: i32,
    step7: u8,
    d: u8,
    /// Known row-length bytes in the sector, and how many the solution reproduces.
    known: usize,
    ok: usize,
    logical: [u8; S7_LEN],
}

/// Known plaintext in sector 7: the u16 row lengths from the slot directory, as (stored index,
/// byte) for rotation `r7`.
fn s7_known(p: &Prepared, r7: usize) -> Vec<(usize, u8)> {
    let starts = p.slot_starts();
    let mut known = Vec::new();
    for (k, &st) in starts.iter().enumerate() {
        let next = starts.get(k + 1).copied().unwrap_or(ROW_AREA_END);
        let len = next.wrapping_sub(st) as u16;
        for (off, b) in [(st, len as u8), (st + 1, (len >> 8) as u8)] {
            if (S7_START..S7_START + S7_LEN).contains(&off) {
                known.push((s7_source(off - S7_START, r7), b));
            }
        }
    }
    known
}

/// Finds the step (when not shared with sector 6) and multiplier that best decode sector 7 with
/// correction row `c_row`: every candidate must reproduce at least half the known row-length
/// bytes, and the most plausible bytes win.
fn solve_s7(p: &Prepared, r7: usize, c_row: &[u8]) -> Option<S7Solution> {
    let known = s7_known(p, r7);
    let mut best: Option<(i32, u8, u8, usize, [u8; S7_LEN])> = None;
    for step7 in p.step7_candidates() {
        let res = p.residual(step7);
        for d in (1..=255u8).step_by(2) {
            let ok = known.iter().filter(|(i, b)| res[*i].wrapping_sub(d.wrapping_mul(c_row[*i])) == *b).count();
            if ok * 2 < known.len() {
                continue;
            }
            // Plausibility ignores byte order, so score the stored order and rearrange the winner.
            let mut plain = [0u8; S7_LEN];
            for i in 0..S7_LEN {
                plain[i] = res[i].wrapping_sub(d.wrapping_mul(c_row[i]));
            }
            let score = plausibility(&plain) + 400 * (2 * ok as i32 - known.len() as i32);
            if best.as_ref().is_none_or(|(b, ..)| score > *b) {
                best = Some((score, step7, d, ok, plain));
            }
        }
    }
    best.map(|(score, step7, d, ok, plain)| S7Solution { score, step7, d, known: known.len(), ok, logical: s7_logical(&plain, r7) })
}

/// A starting correction row for a rotation: vote over the pages that share sector 6's step,
/// using each page's best-fitting multiplier.
fn initial_row(pages: &[&Prepared]) -> Vec<u8> {
    let mut votes = vec![[0u16; (C_MAX + 1) as usize]; S7_LEN];
    for p in pages.iter().filter(|p| p.q == 0) {
        let res = p.residual(p.steps[6]);
        let inv = inverse_mod_256(best_fit_d(&res));
        for (i, &x) in res.iter().enumerate() {
            let c = x.wrapping_mul(inv);
            if c <= C_MAX {
                votes[i][c as usize] += 1;
            }
        }
    }
    votes.iter().map(|v| v.iter().enumerate().max_by_key(|(_, n)| **n).map(|(c, _)| c as u8).unwrap_or(0)).collect()
}

fn best_fit_d(res: &[u8; S7_LEN]) -> u8 {
    (1..=255u8)
        .step_by(2)
        .max_by_key(|&d| {
            let inv = inverse_mod_256(d);
            res.iter().filter(|&&x| x.wrapping_mul(inv) <= C_MAX).count()
        })
        .unwrap_or(1)
}

/// Recovers the cipher of a page whose sector-0 base the page-wide model got wrong (about 1.5%
/// of row pages): searches sector 0's (step, base) for the one under which the page number
/// appears exactly once and the header it anchors is a row-page header, then each of sectors
/// 1..6 for the step (base is sector 0's plus the sector index) that reproduces the most row
/// lengths starting in that sector. Returns base, steps, decoded sectors 0..6 and the anchor.
fn recover_cipher(pn: u64, raw: &[u8]) -> Option<(u8, [u8; 7], Vec<u8>, usize)> {
    let anchor = (pn as u32).to_le_bytes();
    let plain = |base: u8, step: u8, lo: usize, i: usize| {
        raw[lo + i].wrapping_sub(base).wrapping_sub((i as u8).wrapping_mul(step))
    };
    let mut found: Option<(u8, u8, usize)> = None;
    for step0 in 0..=255u8 {
        for base in 0..=255u8 {
            let k = |i: usize| plain(base, step0, 0, i % SECTOR);
            // As in `prepare`, a row can repeat the anchor bytes: check each hit's header.
            for r0 in (1..SECTOR).step_by(2).filter(|&r| (0..4).all(|j| k(r + j) == anchor[j])) {
                let h = |o: usize| k(r0 + o);
                let slots = u16::from_le_bytes([h(22), h(23)]) as usize;
                if h(16) == 4 && matches!(h(17), 0..=4 | 0x10..=0x14) && slots <= MAX_SLOTS {
                    if found.is_some() {
                        return None;
                    }
                    found = Some((base, step0, r0));
                }
            }
        }
    }
    let (base, step0, r0) = found?;

    let mut s0 = vec![0u8; SECTOR];
    for (i, b) in s0.iter_mut().enumerate() {
        *b = plain(base, step0, 0, i);
    }
    let logical0: Vec<u8> = [&s0[r0..], &s0[..r0]].concat();
    let slots = u16le(&logical0, 22) as usize;
    let mut starts: Vec<usize> = (0..slots)
        .filter(|i| SLOT_BASE + 2 * i + 2 <= SECTOR)
        .map(|i| u16le(&logical0, SLOT_BASE + 2 * i) as usize)
        .filter(|&v| v != 0)
        .map(|v| SLOT_BASE + v)
        .collect();
    starts.sort_unstable();

    let mut best: Option<(i64, [u8; 7], Vec<u8>)> = None;
    for q in 0..2usize {
        let mut steps = [step0; 7];
        let mut dec = raw[..S7_START + S7_LEN].to_vec();
        dec[..SECTOR].copy_from_slice(&s0);
        let mut total = 0i64;
        for s in 1..7 {
            let lo = s * SECTOR;
            let rot = (r0 + 2 * SECTOR - 2 * ((s + q) / 2)) % SECTOR;
            let known: Vec<(usize, u16)> = starts
                .iter()
                .enumerate()
                .filter(|(_, &st)| st >= lo && st + 2 <= lo + SECTOR)
                .map(|(k, &st)| (st - lo, (starts.get(k + 1).copied().unwrap_or(ROW_AREA_END) - st) as u16))
                .collect();
            // Sectors in a pair share a step; prefer the partner's when the evidence ties.
            let partner = if (s + q) % 2 == 1 { Some(steps[s - 1]) } else { None };
            let mut pick = (i64::MIN, 0u8);
            for step in 0..=255u8 {
                let b = base.wrapping_add(s as u8);
                let byte = |x: usize| plain(b, step, lo, (x + rot) % SECTOR);
                let ok = known.iter().filter(|(x, len)| u16::from_le_bytes([byte(*x), byte(*x + 1)]) == *len).count() as i64;
                let sec: Vec<u8> = (0..SECTOR).map(byte).collect();
                let score = ok * 1000 + plausibility(&sec) as i64 + if partner == Some(step) { 50 } else { 0 };
                if score > pick.0 {
                    pick = (score, step);
                }
            }
            steps[s] = pick.1;
            total += pick.0 / 1000;
            let b = base.wrapping_add(s as u8);
            for i in 0..SECTOR {
                dec[lo + i] = plain(b, pick.1, lo, i);
            }
        }
        if best.as_ref().is_none_or(|(t, ..)| total > *t) {
            best = Some((total, steps, dec));
        }
    }
    let (_, steps, mut dec) = best?;
    dec.resize(PAGE_SIZE, 0);
    Some((base, steps, dec, r0))
}

/// Re-derives the rotation of sectors 1..6 where the predicted one does not reproduce the
/// lengths of the rows that start in the sector (about 3% of sectors drift by 2, 4 or half a
/// sector). Sector 0 is anchored by the page number and holds the directory, so it is trusted.
fn repair_rotations(p: &mut Prepared, dec: &[u8]) {
    let starts = p.slot_starts();
    for s in 1..7 {
        let lo = s * SECTOR;
        let known: Vec<(usize, u16)> = starts
            .iter()
            .enumerate()
            .filter(|(_, &st)| st >= lo && st + 2 <= lo + SECTOR)
            .map(|(k, &st)| (st - lo, (starts.get(k + 1).copied().unwrap_or(ROW_AREA_END) - st) as u16))
            .collect();
        if known.is_empty() {
            continue;
        }
        let sec = &dec[lo..lo + SECTOR];
        let matches = |rot: usize| {
            known
                .iter()
                .filter(|(x, len)| u16::from_le_bytes([sec[(x + rot) % SECTOR], sec[(x + 1 + rot) % SECTOR]]) == *len)
                .count()
        };
        let predicted = p.rots[s];
        let mut best = (matches(predicted), predicted);
        if best.0 == known.len() {
            continue;
        }
        for rot in 0..SECTOR {
            let m = matches(rot);
            if m > best.0 {
                best = (m, rot);
            }
        }
        if best.0 < known.len() && known.len() >= 2 {
            if let Some((rot, step, base, m)) = solve_sector_cipher(&p.raw[lo..lo + SECTOR], &known) {
                if m > best.0 {
                    p.rots[s] = rot;
                    p.steps[s] = step;
                    if s == 6 {
                        p.base7 = base.wrapping_add(1);
                    }
                    let plain: Vec<u8> = (0..SECTOR)
                        .map(|i| p.raw[lo + i].wrapping_sub(base).wrapping_sub((i as u8).wrapping_mul(step)))
                        .collect();
                    p.head[lo..lo + SECTOR].copy_from_slice(&[&plain[rot..], &plain[..rot]].concat());
                    continue;
                }
            }
        }
        if best.1 != predicted {
            p.rots[s] = best.1;
            p.head[lo..lo + SECTOR].copy_from_slice(&[&sec[best.1..], &sec[..best.1]].concat());
        }
    }
}

/// Solves a sector's rotation, step and base from known row lengths alone (for sectors whose
/// cipher changes inside the page): one row's two length bytes give step and base for each
/// candidate rotation, and the other rows confirm it. Returns (rotation, step, base, matches).
fn solve_sector_cipher(raw: &[u8], known: &[(usize, u16)]) -> Option<(usize, u8, u8, usize)> {
    let mut best: Option<(usize, u8, u8, usize)> = None;
    for rot in 0..SECTOR {
        for &(x, len) in known.iter().take(3) {
            let (p0, p1) = ((x + rot) % SECTOR, (x + 1 + rot) % SECTOR);
            if p1 != p0 + 1 {
                continue;
            }
            let [lo_b, hi_b] = len.to_le_bytes();
            let step = raw[p1].wrapping_sub(raw[p0]).wrapping_sub(hi_b.wrapping_sub(lo_b));
            let base = raw[p0].wrapping_sub(lo_b).wrapping_sub((p0 as u8).wrapping_mul(step));
            let byte = |i: usize| raw[i].wrapping_sub(base).wrapping_sub((i as u8).wrapping_mul(step));
            let m = known
                .iter()
                .filter(|(x, len)| u16::from_le_bytes([byte((x + rot) % SECTOR), byte((x + 1 + rot) % SECTOR)]) == *len)
                .count();
            if m >= 2 && best.as_ref().is_none_or(|b| m > b.3) {
                best = Some((rot, step, base, m));
            }
        }
    }
    best
}

/// Header validity plus the number of rows in sectors 0..6 whose length chains; negative when
/// the header is not a row-page header.
fn head_score(p: &Prepared) -> i64 {
    let h = &p.head;
    let n = u16le(h, 22) as usize;
    if h[16] != 4 || n > MAX_SLOTS || SLOT_BASE + 2 * n > S7_START {
        return -1;
    }
    let starts = p.slot_starts();
    let mut ok = 0;
    for (k, &st) in starts.iter().enumerate() {
        let next = starts.get(k + 1).copied().unwrap_or(ROW_AREA_END);
        if st + 2 <= S7_START && u16le(h, st) as usize == next.wrapping_sub(st) {
            ok += 1;
        }
    }
    ok
}

/// Test support: reads the page cache the real-file test writes (see `decodes_pages_of_a_real_file`).
#[cfg(test)]
pub fn load_page_cache(path: &str) -> Vec<LogicalPage> {
    let d = std::fs::read(path).expect("page cache");
    let mut o = 0;
    let mut pages = Vec::new();
    while o < d.len() {
        let number = u64::from_le_bytes(d[o..o + 8].try_into().unwrap());
        let kind = d[o + 8];
        let owner = u32le(&d, o + 9);
        let sector7 = match d[o + 13] {
            0 => Sector7::Verified,
            1 => Sector7::BestEffort,
            _ => Sector7::Unsolved,
        };
        o += 14;
        let bytes = d[o..o + ROW_AREA_END].to_vec();
        o += ROW_AREA_END;
        let n = u16le(&d, o) as usize;
        o += 2;
        let rows = (0..n)
            .map(|i| {
                let b = o + 5 * i;
                RowSpan { start: u16le(&d, b), len: u16le(&d, b + 2), chained: d[b + 4] == 1 }
            })
            .collect();
        o += 5 * n;
        pages.push(LogicalPage { number, kind, owner, bytes, rows, sector7, q: 0, r7: 0, c_exact: None });
    }
    pages
}

#[cfg(test)]
mod tests;
