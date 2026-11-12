use super::*;

#[test]
fn inverse_is_exact_for_every_odd_byte() {
    for d in (1..=255u8).step_by(2) {
        assert_eq!(d.wrapping_mul(inverse_mod_256(d)), 1, "d = {d}");
    }
}

#[test]
fn sector7_mapping_moves_the_displaced_block_into_the_trailer_window() {
    // No rotation: the trailer window starts at 496, so nothing is displaced.
    assert_eq!(s7_source(0, 0), 0);
    assert_eq!(s7_source(495, 0), 495);
    // r7 = 100: logical 0..396 is stored 100..496, the window 396..412 holds the displaced
    // stored bytes 84..100, and the rest is stored 0..84.
    assert_eq!(s7_source(0, 100), 100);
    assert_eq!(s7_source(395, 100), 495);
    assert_eq!(s7_source(396, 100), 84);
    assert_eq!(s7_source(411, 100), 99);
    assert_eq!(s7_source(412, 100), 0);
    assert_eq!(s7_source(495, 100), 83);
    let all: std::collections::BTreeSet<usize> = (0..S7_LEN).map(|x| s7_source(x, 100)).collect();
    assert_eq!(all.len(), S7_LEN);
}

#[test]
fn sector7_mapping_handles_rotations_next_to_the_sector_boundary() {
    // Every rotation maps the 496 logical bytes onto the 496 stored data bytes.
    for r7 in 0..SECTOR {
        let all: std::collections::BTreeSet<usize> = (0..S7_LEN).map(|x| s7_source(x, r7)).collect();
        assert_eq!((all.len(), all.last().copied()), (S7_LEN, Some(S7_LEN - 1)), "r7 = {r7}");
    }
    // r7 = 8: the trailer slot wraps, so only 8 bytes are displaced; they open the sector.
    assert_eq!(s7_source(0, 8), 8);
    assert_eq!(s7_source(487, 8), 495);
    assert_eq!((488..S7_LEN).map(|x| s7_source(x, 8)).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
    // r7 = 505: the first 7 logical bytes land on the stored trailer. Each moves on by the
    // rotation, to where the trailer byte it displaced went, until it is below the trailer: the
    // first five in two steps, the last two in three (gap R3d).
    assert_eq!((0..7).map(|x| s7_source(x, 505)).collect::<Vec<_>>(), [491, 492, 493, 494, 495, 489, 490]);
    assert_eq!(s7_source(7, 505), 0);
    assert_eq!(s7_source(495, 505), 488);
    // r7 = 509, as on a textbook file's inventory adjustment line: logical byte 2 (the low byte
    // of the account id) is stored at 493, before bytes 0 and 1. Read in order, the line had its
    // transaction id and account wrong and the adjustment did not balance.
    assert_eq!((0..3).map(|x| s7_source(x, 509)).collect::<Vec<_>>(), [494, 495, 493]);
}

/// Why named pages (LOCALBOOKS_QBW_PAGES_PROBE) fail to decode: anchor hits in the model's
/// sector 0, and every (step0, base, r0) recover_cipher would consider, with its header bytes.
#[test]
#[ignore]
fn probes_undecoded_page() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let pn: u64 = std::env::var("LOCALBOOKS_QBW_PAGES_PROBE").unwrap().parse().unwrap();
    let dec = PageDecoder::open(std::path::Path::new(&path)).unwrap();
    let raw = dec.store.page(pn).unwrap().bytes().to_vec();
    let model = match recover_bv_any(pn, &raw) {
        Some(bv) => deobfuscate_with_bv(&raw, pn, bv),
        None => dec.model.deobfuscate_with_store(&raw, pn, &dec.store),
    };
    let anchor = (pn as u32).to_le_bytes();
    let hits: Vec<usize> = (0..SECTOR).filter(|&i| (0..4).all(|k| model[(i + k) % SECTOR] == anchor[k])).collect();
    println!("page {pn}: model anchor hits {hits:?}");
    let mut cands = Vec::new();
    for step0 in 0..=255u8 {
        for base in 0..=255u8 {
            let k = |i: usize| raw[i % SECTOR].wrapping_sub(base).wrapping_sub(((i % SECTOR) as u8).wrapping_mul(step0));
            let hits: Vec<usize> = (1..SECTOR).step_by(2).filter(|&r| (0..4).all(|j| k(r + j) == anchor[j])).collect();
            if hits.len() == 1 {
                let r0 = hits[0];
                let h: Vec<u8> = (0..32).map(|o| k(r0 + o)).collect();
                cands.push((step0, base, r0, h));
            }
        }
    }
    println!("  unique-anchor candidates: {}", cands.len());
    for (step0, base, r0, h) in cands.iter().filter(|c| c.3[16] == 4).take(10) {
        println!("  step0 {step0} base {base} r0 {r0} header {:02x?}", h);
    }
}

/// Lists the row pages that do not decode at all, with the owners of their decoded neighbours.
#[test]
#[ignore]
fn lists_undecoded_pages() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let mut dec = PageDecoder::open(std::path::Path::new(&path)).unwrap();
    let (pages, _) = dec.decode_all();
    let owner: std::collections::HashMap<u64, (u32, u8)> = pages.iter().map(|p| (p.number, (p.owner, p.kind))).collect();
    for pn in dec.row_page_numbers() {
        if owner.contains_key(&pn) {
            continue;
        }
        let near: Vec<String> = (pn.saturating_sub(3)..=pn + 3).filter(|n| *n != pn).map(|n| format!("{n}:{:?}", owner.get(&n))).collect();
        let raw = dec.store.page(pn).unwrap().bytes().to_vec();
        println!("page {pn} undecoded; trailer type {:?}; live rows {}; neighbours {}", raw[0xFF2] as char, u16le(&raw, 0xFF0), near.join(" "));
    }
}

/// For the pages named in LOCALBOOKS_QBW_PAGES_PROBE (comma-separated), counts the sector-7
/// solutions that reproduce every known row-length byte, over every learned correction row
/// and every rotation.
#[test]
#[ignore]
fn probes_sector7_of_named_pages() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let wanted: Vec<u64> = std::env::var("LOCALBOOKS_QBW_PAGES_PROBE").unwrap().split(',').map(|s| s.trim().parse().unwrap()).collect();
    let mut dec = PageDecoder::open(std::path::Path::new(&path)).unwrap();
    let _ = dec.decode_all();
    for pn in wanted {
        let Some(p) = dec.prepare(pn) else {
            println!("page {pn}: no anchor");
            continue;
        };
        let starts = p.slot_starts();
        let s7_rows: Vec<usize> = starts.iter().copied().filter(|&s| s + 2 > S7_START).collect();
        println!("page {pn}: q {} r0 {} rots {:?} r7 {} steps {:?} base7 {} s7 row starts {:?}", p.q, p.r0, p.rots, p.r7(), p.steps, p.base7, s7_rows);
        let mut hits = Vec::new();
        for rv in (1..SECTOR).step_by(2) {
            let known = s7_known(&p, rv);
            for (&key, (c_row, exact)) in &dec.c_rows {
                for step7 in p.step7_candidates() {
                    let res = p.residual(step7);
                    for d in (1..=255u8).step_by(2) {
                        if known.iter().all(|(i, b)| res[*i].wrapping_sub(d.wrapping_mul(c_row[*i])) == *b) {
                            hits.push((rv, key, *exact, step7, d));
                        }
                    }
                }
            }
            let _ = known;
        }
        println!("  known bytes {} -> {} candidate solutions; first: {:?}", s7_known(&p, p.r7()).len(), hits.len(), &hits[..hits.len().min(12)]);
        // Same search, letting sector 7's base float: the first known byte fixes it.
        let raw7 = &p.raw[S7_START..S7_START + S7_LEN];
        let mut free = Vec::new();
        for rv in (1..SECTOR).step_by(2) {
            let known = s7_known(&p, rv);
            let Some(&(i0, b0)) = known.first() else { continue };
            for (&key, (c_row, exact)) in &dec.c_rows {
                for step7 in 0..=255u8 {
                    for d in (1..=255u8).step_by(2) {
                        let base7 = raw7[i0].wrapping_sub((i0 as u8).wrapping_mul(step7)).wrapping_sub(d.wrapping_mul(c_row[i0])).wrapping_sub(b0);
                        if known.iter().all(|(i, b)| {
                            raw7[*i].wrapping_sub(base7).wrapping_sub((*i as u8).wrapping_mul(step7)).wrapping_sub(d.wrapping_mul(c_row[*i])) == *b
                        }) {
                            free.push((rv, key, *exact, step7, d, base7));
                        }
                    }
                }
            }
        }
        println!("  with a free base: {} candidates; first: {:?}", free.len(), &free[..free.len().min(12)]);
    }
}

/// Decode statistics for a real company file (read-only): set LOCALBOOKS_QBW_CHECK and run
/// `cargo test --release -- --ignored decodes_pages_of_a_real --nocapture`.
#[test]
#[ignore]
fn decodes_pages_of_a_real_file() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let started = std::time::Instant::now();
    let mut decoder = PageDecoder::open(std::path::Path::new(&path)).unwrap();
    let (pages, stats) = decoder.decode_all();
    println!("{stats:#?}");
    let (mut s7_rows, mut s7_ok) = (0, 0);
    for p in &pages {
        for r in &p.rows {
            if r.start as usize >= S7_START {
                s7_rows += 1;
                s7_ok += r.chained as u32;
            }
        }
    }
    let mut by: std::collections::BTreeMap<String, (u32, u32)> = Default::default();
    for p in &pages {
        let key = format!("q{} c={:?} s7={:?}", p.q, p.c_exact, p.sector7);
        for r in p.rows.iter().filter(|r| r.start as usize >= S7_START) {
            let e = by.entry(key.clone()).or_default();
            e.0 += r.chained as u32;
            e.1 += 1;
        }
    }
    for (k, (ok, n)) in &by {
        println!("  {k:<45} {ok:>6}/{n:<6}");
    }
    let mut unsolved: std::collections::BTreeMap<u16, u32> = Default::default();
    for p in pages.iter().filter(|p| p.sector7 == Sector7::Unsolved) {
        *unsolved.entry(p.r7).or_default() += 1;
    }
    println!("unsolved pages by predicted r7: {unsolved:?}");
    if let Ok(out) = std::env::var("LOCALBOOKS_QBW_PAGES_OUT") {
        // Cache for offline analysis: per page u64 number, u8 kind, u32 owner, u8 sector-7
        // status, 4092 logical bytes, u16 row count, then (u16 start, u16 len, u8 chained).
        let mut buf = Vec::new();
        for p in &pages {
            buf.extend_from_slice(&p.number.to_le_bytes());
            buf.push(p.kind);
            buf.extend_from_slice(&p.owner.to_le_bytes());
            buf.push(p.sector7 as u8);
            buf.extend_from_slice(&p.bytes);
            buf.extend_from_slice(&(p.rows.len() as u16).to_le_bytes());
            for r in &p.rows {
                buf.extend_from_slice(&r.start.to_le_bytes());
                buf.extend_from_slice(&r.len.to_le_bytes());
                buf.push(r.chained as u8);
            }
        }
        std::fs::write(&out, buf).unwrap();
    }
    println!("rows starting in sector 7: {s7_ok}/{s7_rows} chain; {} pages in {:.1}s", pages.len(), started.elapsed().as_secs_f64());
}

// Synthetic pages for the decoder's regression tests: a logical row page, stored the way the
// page store keeps it (each sector rotated and enciphered with its own base and step). The rows
// and values are made up for these tests.

const PN: u64 = 0x0003_1A27;
const R0: usize = 157;
const BASE: u8 = 0x5B;
const STEPS: [u8; 7] = [3, 3, 9, 9, 21, 21, 37];

/// Rows start in every sector, and two of them in sector 7; the last runs to the end of the row
/// area. The row at 100 holds the page number at logical offset 200, which lands on an odd
/// stored offset of sector 0, like the real page whose row id equalled its page number.
const ROW_STARTS: [usize; 9] = [100, 600, 1100, 1600, 2100, 2600, 3100, 3700, 3900];

fn logical_page() -> Vec<u8> {
    let mut l = vec![0u8; PAGE_SIZE];
    l[..4].copy_from_slice(&(PN as u32).to_le_bytes());
    l[16] = 4;
    l[22..24].copy_from_slice(&(ROW_STARTS.len() as u16).to_le_bytes());
    l[24..28].copy_from_slice(&812u32.to_le_bytes());
    for (k, &st) in ROW_STARTS.iter().enumerate() {
        l[SLOT_BASE + 2 * k..SLOT_BASE + 2 * k + 2].copy_from_slice(&((st - SLOT_BASE) as u16).to_le_bytes());
        let next = ROW_STARTS.get(k + 1).copied().unwrap_or(ROW_AREA_END);
        l[st..st + 2].copy_from_slice(&((next - st) as u16).to_le_bytes());
        // Row text, kept clear of the last 12 bytes (the page header stashes those).
        for (i, b) in l[st + 8..(next - 8).min(4080)].iter_mut().enumerate() {
            *b = if i % 7 == 0 { 0 } else { b'0' + (i % 10) as u8 };
        }
    }
    l[200..204].copy_from_slice(&(PN as u32).to_le_bytes());
    l
}

fn rotation(s: usize) -> usize {
    (R0 + 2 * SECTOR - 2 * (s / 2)) % SECTOR
}

/// Stores `l`: returns (raw page, the page-wide decode of sectors 0..6). Sector 7 uses its own
/// base `base7` and no correction term.
fn store(l: &[u8], base7: u8) -> (Vec<u8>, Vec<u8>) {
    let mut raw = vec![0u8; PAGE_SIZE];
    let mut dec = vec![0u8; PAGE_SIZE];
    for s in 0..7 {
        let rot = rotation(s);
        for k in 0..SECTOR {
            let plain = l[s * SECTOR + (k + SECTOR - rot) % SECTOR];
            dec[s * SECTOR + k] = plain;
            raw[s * SECTOR + k] = plain.wrapping_add(BASE.wrapping_add(s as u8)).wrapping_add((k as u8).wrapping_mul(STEPS[s]));
        }
    }
    let r7 = rotation(6);
    let mut plain7 = [0u8; S7_LEN];
    for x in 0..S7_LEN {
        plain7[s7_source(x, r7)] = l[S7_START + x];
    }
    for (i, &b) in plain7.iter().enumerate() {
        raw[S7_START + i] = b.wrapping_add(base7).wrapping_add((i as u8).wrapping_mul(STEPS[6]));
    }
    (raw, dec)
}

#[test]
fn a_row_that_repeats_the_page_number_does_not_hide_the_page() {
    let l = logical_page();
    let (raw, dec) = store(&l, BASE.wrapping_add(7));
    let anchors = (1..SECTOR).step_by(2).filter(|&i| (0..4).all(|k| dec[(i + k) % SECTOR] == (PN as u32).to_le_bytes()[k])).count();
    assert_eq!(anchors, 2, "the page number appears twice in sector 0");

    // With the page-wide decode: each anchor is tried and the real header wins.
    let p = lay_out(PN, raw.clone(), dec).expect("the page is laid out");
    assert_eq!(p.r0, R0);
    assert_eq!(p.head, l[..S7_START]);

    // When the page-wide decode is wrong, the cipher is recovered from the page alone.
    let p = lay_out(PN, raw, vec![0u8; PAGE_SIZE]).expect("the cipher is recovered");
    assert_eq!((p.r0, p.steps), (R0, STEPS));
    assert_eq!(p.head, l[..S7_START]);
}

#[test]
fn sector7_with_its_own_cipher_base_is_solved_from_the_row_lengths() {
    let l = logical_page();
    let (raw, dec) = store(&l, BASE.wrapping_add(7).wrapping_add(50));
    let p = lay_out(PN, raw, dec).unwrap();
    assert_ne!(p.base7, BASE.wrapping_add(7).wrapping_add(50), "the base the other sectors imply is not sector 7's");

    // The first decode, with the assumed base: the two rows in sector 7 do not chain.
    let mut bytes = p.head.clone();
    bytes.resize(ROW_AREA_END, 0);
    bytes[S7_START + S7_LEN..ROW_AREA_END].copy_from_slice(&[&p.head[18..20], &p.head[6..16]].concat());
    let rows: Vec<RowSpan> = ROW_STARTS
        .iter()
        .enumerate()
        .map(|(k, &st)| {
            let len = (ROW_STARTS.get(k + 1).copied().unwrap_or(ROW_AREA_END) - st) as u16;
            RowSpan { start: st as u16, len, chained: u16le(&bytes, st) == len }
        })
        .collect();
    let page = LogicalPage { number: PN, kind: 0, owner: 812, bytes, rows, sector7: Sector7::BestEffort, q: 0, r7: p.r7() as u16, c_exact: Some(true) };
    let chained = |pg: &LogicalPage| pg.rows.iter().filter(|r| r.chained).count();
    assert_eq!(chained(&page), ROW_STARTS.len() - 2);

    let c_rows = HashMap::from([(p.r7(), (vec![0u8; S7_LEN], true))]);
    let fixed = refine_sector7(&p, &c_rows, &page, &chained).expect("a better sector 7");
    assert_eq!(chained(&fixed), ROW_STARTS.len());
    assert_eq!(fixed.bytes, l[..ROW_AREA_END]);
}

/// How the learned sector-7 correction rows relate: for each estimated row, the exact row and
/// shift that agree with it most, and between exact rows, the shifts that make them equal.
#[test]
#[ignore]
fn compares_correction_rows() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let mut dec = PageDecoder::open(std::path::Path::new(&path)).unwrap();
    let _ = dec.decode_all();
    let mut exact: Vec<(usize, Vec<u8>)> = dec.c_rows.iter().filter(|(_, (_, e))| *e).map(|(r, (row, _))| (*r, row.clone())).collect();
    exact.sort();
    let estimated: Vec<(usize, Vec<u8>)> = dec.c_rows.iter().filter(|(_, (_, e))| !*e).map(|(r, (row, _))| (*r, row.clone())).collect();
    println!("exact rotations {:?}", exact.iter().map(|e| e.0).collect::<Vec<_>>());
    println!("estimated rotations {:?}", estimated.iter().map(|e| e.0).collect::<Vec<_>>());
    for (r, row) in exact.iter() {
        println!("exact r7 {r}: {}", row.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","));
    }
    for (r, row) in estimated.iter() {
        println!("estimated r7 {r}: {}", row.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","));
    }
    // Do two exact rows agree under a shift?
    for w in exact.windows(2) {
        let ((ra, a), (rb, b)) = (&w[0], &w[1]);
        let best = (0..S7_LEN).map(|k| ((0..S7_LEN - k).filter(|&i| a[i + k] == b[i]).count(), k)).max().unwrap();
        let best_neg = (0..S7_LEN).map(|k| ((0..S7_LEN - k).filter(|&i| a[i] == b[i + k]).count(), k)).max().unwrap();
        println!("r7 {ra} vs {rb}: best a[i+k]==b[i] {best:?}; a[i]==b[i+k] {best_neg:?}");
    }
}

/// Votes for an estimated correction row are pooled over positions one rotation apart: a value
/// seen once at each of several positions of the same phase beats a stray value seen twice at one
/// position (gap R3d: rows in sector 7 of such pages read with wrong bytes).
#[test]
fn estimated_correction_rows_pool_votes_one_period_apart() {
    let mut votes = vec![[0u16; (C_MAX + 1) as usize]; S7_LEN];
    votes[0][16] = 1;
    // Rotation 5: positions 3, 8, 13 share a phase. Each saw 7 once; position 8 saw 2 twice.
    for i in [3, 13] {
        votes[i][7] = 1;
    }
    votes[8][7] = 1;
    votes[8][2] = 2;
    let pooled = pool_periodic_votes(votes.clone(), 5);
    let best = |v: &[u16; (C_MAX + 1) as usize]| v.iter().enumerate().max_by_key(|(_, n)| **n).map(|(c, _)| c);
    assert_eq!(best(&votes[8]), Some(2));
    assert_eq!(best(&pooled[8]), Some(7));
    assert_eq!((pooled[3], pooled[13]), (pooled[8], pooled[8]));
    assert_eq!(pooled[0], votes[0], "the first position keeps its own votes");
    // From half a sector up, rows have no such period: the votes are left alone.
    assert_eq!(pool_periodic_votes(votes.clone(), 300), votes);
}
