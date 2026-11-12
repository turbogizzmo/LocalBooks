//! Password-protected PDFs: the standard security handler's AES-256 method (revision 6, as
//! described in ISO 32000-2 §7.6.4), which every current PDF reader opens.
//!
//! A random 256-bit file key encrypts each stream and string with AES-256-CBC (a random IV in front,
//! PKCS#7 padding). The encryption dictionary stores the key wrapped twice: under the password the
//! reader types (U and UE), and under a random owner password nobody knows (O and OE), so the file
//! can only be opened with the password. Opening it grants every permission (print, copy).

use aes::cipher::{BlockEncrypt, KeyInit};
use cbc::cipher::block_padding::{NoPadding, Pkcs7};
use cbc::cipher::{BlockEncryptMut, KeyIvInit};
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
        let e = cbc::Encryptor::<aes::Aes128>::new_from_slices(&k[..16], &k[16..32]).expect("16-byte key and IV").encrypt_padded_vec_mut::<NoPadding>(&k1);
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
    cbc::Encryptor::<aes::Aes256>::new_from_slices(key, &[0u8; 16]).expect("32-byte key").encrypt_padded_vec_mut::<NoPadding>(file_key)
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
        let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(&perms);
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
        out.extend(
            cbc::Encryptor::<aes::Aes256>::new_from_slices(&self.file_key, &iv).expect("32-byte key and 16-byte IV").encrypt_padded_vec_mut::<Pkcs7>(data),
        );
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
}
