//! Password-protected PDFs: the standard security handler's AES-256 method (revision 6, as
//! described in ISO 32000-2 §7.6.4), which every current PDF reader opens.
//!
//! A random 256-bit file key encrypts each stream and string with AES-256-CBC (a random IV in front,
//! PKCS#7 padding). The encryption dictionary stores the key wrapped twice: under the password the
//! reader types (U and UE), and under a random owner password nobody knows (O and OE), so the file
//! can only be opened with the password. Opening it grants every permission (print, copy).

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use cbc::cipher::block_padding::{NoPadding, Pkcs7};
use cbc::cipher::{BlockModeEncrypt, KeyIvInit};
use sha2::{Digest, Sha256, Sha384, Sha512};

/// Every permission: bits 1–2 must be 0, the rest set (ISO 32000-2, Table 22).
const PERMISSIONS: i32 = -4;

/// The longest password, in UTF-8 bytes, that the method uses (longer ones are cut there).
pub const MAX_PASSWORD_BYTES: usize = 127;

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("PDF_RANDOM: the system's random number generator failed ({e})"))?;
    Ok(bytes)
}

/// The revision 6 password hash (ISO 32000-2 §7.6.4.3.4, algorithm 2.B).
fn hash(password: &[u8], salt: &[u8], user_key: &[u8]) -> [u8; 32] {
    let mut k: Vec<u8> = Sha256::new().chain_update(password).chain_update(salt).chain_update(user_key).finalize().to_vec();
    let mut round = 0usize;
    let mut last = 0u8;
    while round < 64 || usize::from(last) + 32 > round {
        let mut k1 = Vec::with_capacity(64 * (password.len() + k.len() + user_key.len()));
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(user_key);
        }
        let e = cbc::Encryptor::<aes::Aes128>::new_from_slices(&k[..16], &k[16..32]).expect("16-byte key and IV").encrypt_padded_vec::<NoPadding>(&k1);
        // The first 16 bytes as a number, mod 3: 256 ≡ 1 (mod 3), so the byte sum mod 3.
        let pick = e[..16].iter().map(|&b| u32::from(b)).sum::<u32>() % 3;
        k = match pick {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        last = *e.last().expect("never empty");
        round += 1;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&k[..32]);
    out
}

/// AES-256-CBC with a zero IV and no padding: how UE and OE wrap the 32-byte file key.
fn wrap_key(key: &[u8; 32], file_key: &[u8; 32]) -> Vec<u8> {
    cbc::Encryptor::<aes::Aes256>::new_from_slices(key, &[0u8; 16]).expect("32-byte key").encrypt_padded_vec::<NoPadding>(file_key)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2 + 2);
    out.push('<');
    for b in bytes {
        out.push_str(&format!("{b:02X}"));
    }
    out.push('>');
    out
}

/// One file's encryption: its key and the dictionary a reader needs to open it.
pub struct Protection {
    file_key: [u8; 32],
    /// Counts IVs made without the system's generator (see `encrypt`).
    fallback: std::cell::Cell<u64>,
    /// The `/Encrypt` dictionary.
    pub dictionary: String,
    /// The file identifier for the trailer's `/ID`.
    pub id: String,
}

impl Protection {
    /// A password the method can use: not empty, and at most 127 bytes.
    pub fn check_password(password: &str) -> Result<(), String> {
        if password.is_empty() {
            return Err("PDF_PASSWORD_EMPTY: enter a password for the PDF".into());
        }
        if password.len() > MAX_PASSWORD_BYTES {
            return Err(format!(
                "PDF_PASSWORD_TOO_LONG: a PDF password can be at most {MAX_PASSWORD_BYTES} bytes: {MAX_PASSWORD_BYTES} plain letters and digits, fewer accented or non-Latin characters"
            ));
        }
        Ok(())
    }

    pub fn new(password: &str) -> Result<Protection, String> {
        Self::check_password(password)?;
        let user = password.as_bytes();
        let owner: [u8; 32] = random()?;
        let file_key: [u8; 32] = random()?;
        let salts: [u8; 32] = random()?;
        let (u_valid, u_key, o_valid, o_key) = (&salts[0..8], &salts[8..16], &salts[16..24], &salts[24..32]);

        let mut u = hash(user, u_valid, &[]).to_vec();
        u.extend_from_slice(u_valid);
        u.extend_from_slice(u_key);
        let ue = wrap_key(&hash(user, u_key, &[]), &file_key);

        let mut o = hash(&owner, o_valid, &u).to_vec();
        o.extend_from_slice(o_valid);
        o.extend_from_slice(o_key);
        let oe = wrap_key(&hash(&owner, o_key, &u), &file_key);

        // Perms: the permissions again, sealed with the file key so they can't be changed.
        let tail: [u8; 4] = random()?;
        let mut perms = [0u8; 16];
        perms[..4].copy_from_slice(&PERMISSIONS.to_le_bytes());
        perms[4..8].copy_from_slice(&[0xFF; 4]);
        perms[8..12].copy_from_slice(b"Tadb");
        perms[12..].copy_from_slice(&tail);
        let mut block = aes::Block::from(perms);
        aes::Aes256::new_from_slice(&file_key).expect("32-byte key").encrypt_block(&mut block);

        let dictionary = format!(
            "<< /Filter /Standard /V 5 /R 6 /Length 256 /CF << /StdCF << /Type /CryptFilter /CFM /AESV3 /AuthEvent /DocOpen /Length 32 >> >> \
             /StmF /StdCF /StrF /StdCF /O {} /U {} /OE {} /UE {} /P {PERMISSIONS} /Perms {} /EncryptMetadata true >>",
            hex(&o),
            hex(&u),
            hex(&oe),
            hex(&ue),
            hex(&block),
        );
        let id = hex(&random::<16>()?);
        Ok(Protection { file_key, fallback: std::cell::Cell::new(0), dictionary, id })
    }

    /// A stream's or string's bytes, encrypted: a random IV, then AES-256-CBC with PKCS#7 padding.
    pub fn encrypt(&self, data: &[u8]) -> Vec<u8> {
        let iv: [u8; 16] = random().unwrap_or_else(|_| {
            // Should the system's generator fail mid-file, each IV must still be unpredictable and
            // never repeat: hash the secret file key with a count that rises on every call.
            let n = self.fallback.get().wrapping_add(1);
            self.fallback.set(n);
            let mut iv = [0u8; 16];
            iv.copy_from_slice(&Sha256::new().chain_update(self.file_key).chain_update(n.to_le_bytes()).finalize()[..16]);
            iv
        });
        let mut out = iv.to_vec();
        out.extend(cbc::Encryptor::<aes::Aes256>::new_from_slices(&self.file_key, &iv).expect("32-byte key and 16-byte IV").encrypt_padded_vec::<Pkcs7>(data));
        out
    }

    /// A string object (`<…>`) holding `bytes`, encrypted.
    pub fn string(&self, bytes: &[u8]) -> String {
        hex(&self.encrypt(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_hash_takes_at_least_64_rounds_and_is_repeatable() {
        // Made up at run time: no fixed password or salt sits in the source.
        let (password, salt): ([u8; 12], [u8; 8]) = (random().unwrap(), random().unwrap());
        let a = hash(&password, &salt, &[]);
        assert_eq!(a, hash(&password, &salt, &[]));
        let mut other_salt = salt;
        other_salt[0] ^= 1;
        assert_ne!(a, hash(&password, &other_salt, &[]));
        let mut other_password = password;
        other_password[0] ^= 1;
        assert_ne!(a, hash(&other_password, &salt, &[]));
    }

    #[test]
    fn the_password_must_be_given_and_not_too_long() {
        // Letters made up at run time, none, then 128, then 127 of them (no fixed password in code).
        let letters: String = random::<128>().unwrap().iter().map(|b| char::from(b'a' + b % 26)).collect();
        assert!(Protection::check_password(&letters[..0]).unwrap_err().starts_with("PDF_PASSWORD_EMPTY"));
        assert!(Protection::new(&letters).err().unwrap().starts_with("PDF_PASSWORD_TOO_LONG"));
        let p = Protection::new(&letters[..127]).unwrap();
        assert!(p.dictionary.contains("/V 5 /R 6") && p.dictionary.contains("/AESV3"));
        // Each string gets its own IV.
        assert_ne!(p.encrypt(b"same"), p.encrypt(b"same"));
        assert_eq!(p.encrypt(b"0123456789abcdef").len(), 16 + 32, "IV, then a full block of padding");
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    // Known-answer tests: the published vectors for each primitive the method is built on, so a
    // change of cipher or hash crate that alters any output fails here.

    #[test]
    fn aes_256_matches_fips_197() {
        // FIPS 197, Appendix C.3.
        let key = unhex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let mut block = aes::Block::try_from(&unhex("00112233445566778899aabbccddeeff")[..]).unwrap();
        aes::Aes256::new_from_slice(&key).unwrap().encrypt_block(&mut block);
        assert_eq!(block.to_vec(), unhex("8ea2b7ca516745bfeafc49904b496089"));
    }

    #[test]
    fn aes_cbc_matches_nist_sp_800_38a() {
        // SP 800-38A, F.2.1 (CBC-AES128) and F.2.5 (CBC-AES256), the first two blocks of each.
        let iv = unhex("000102030405060708090a0b0c0d0e0f");
        let plain = unhex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51");
        let key128 = unhex("2b7e151628aed2a6abf7158809cf4f3c");
        let e = cbc::Encryptor::<aes::Aes128>::new_from_slices(&key128, &iv).unwrap().encrypt_padded_vec::<NoPadding>(&plain);
        assert_eq!(e, unhex("7649abac8119b246cee98e9b12e9197d5086cb9b507219ee95db113a917678b2"));
        let key256 = unhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4");
        let e = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key256, &iv).unwrap().encrypt_padded_vec::<NoPadding>(&plain);
        assert_eq!(e, unhex("f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d"));
        // PKCS#7 on a whole block: the same first block, then one more block of padding.
        let e = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key256, &iv).unwrap().encrypt_padded_vec::<Pkcs7>(&plain[..16]);
        assert_eq!((e.len(), &e[..16]), (32, &unhex("f58c4c04d6e5f1ba779eabfb5f7bfbd6")[..]));
    }

    #[test]
    fn the_key_wrap_is_cbc_with_a_zero_iv() {
        // With a zero IV the first block is plain AES: SP 800-38A, F.1.5 (ECB-AES256), block 1.
        let key: [u8; 32] = unhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4").try_into().unwrap();
        let mut file_key = [0u8; 32];
        file_key[..16].copy_from_slice(&unhex("6bc1bee22e409f96e93d7e117393172a"));
        let wrapped = wrap_key(&key, &file_key);
        assert_eq!((wrapped.len(), &wrapped[..16]), (32, &unhex("f3eed1bdb5d2a03c064b5a7e3db181f8")[..]));
    }

    #[test]
    fn sha_2_matches_fips_180_4() {
        // The "abc" examples from FIPS 180-4's published example values.
        assert_eq!(Sha256::digest(b"abc").to_vec(), unhex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert_eq!(Sha384::digest(b"abc").to_vec(), unhex("cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"));
        assert_eq!(
            Sha512::digest(b"abc").to_vec(),
            unhex("ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f")
        );
    }
}
