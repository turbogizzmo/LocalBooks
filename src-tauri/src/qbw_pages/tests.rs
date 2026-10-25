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
        let Some(p) = dec.prepare(pn) else { println!("page {pn}: no anchor"); continue };
        let starts = p.slot_starts();
        let s7_rows: Vec<usize> = starts.iter().copied().filter(|&s| s + 2 > S7_START).collect();
        println!("page {pn}: q {} r0 {} rots {:?} r7 {} steps {:?} base7 {} s7 row starts {:?}", p.q, p.r0, p.rots, p.r7(), p.steps, p.base7, s7_rows);
        let mut hits = Vec::new();
        for rv in (1..SECTOR).step_by(2) {
            if !s7_solvable(rv) {
                continue;
            }
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
            if !s7_solvable(rv) {
                continue;
            }
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
