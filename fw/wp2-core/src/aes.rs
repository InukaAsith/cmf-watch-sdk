// AES-128-CBC for the stub's own channel key K (docs/abi-1.4.md). Same sealing as the stock frames and the host's
// `link.encrypt`: plain ‖ crc32le, PKCS7, CBC with the stock fixed IV. Software AES: the stock helpers only use
// the stock key buffers.
use crate::pkg::crc32;

pub const IV: [u8; 16] = [0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x5A];

const fn xtime(x: u8) -> u8 {
    (x << 1) ^ if x & 0x80 != 0 { 0x1B } else { 0 }
}

const fn mul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0;
    while b != 0 {
        if b & 1 != 0 {
            p ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    p
}

const fn sbox() -> [u8; 256] {
    let mut s = [0u8; 256];
    let mut x = 0;
    while x < 256 {
        // multiplicative inverse by brute force (const, runs at compile time)
        let mut inv = 0u8;
        if x != 0 {
            let mut y = 1;
            while y < 256 {
                if mul(x as u8, y as u8) == 1 {
                    inv = y as u8;
                }
                y += 1;
            }
        }
        let b = inv;
        s[x] = b ^ b.rotate_left(1) ^ b.rotate_left(2) ^ b.rotate_left(3) ^ b.rotate_left(4) ^ 0x63;
        x += 1;
    }
    s
}

const SBOX: [u8; 256] = sbox();
const INV: [u8; 256] = {
    let mut inv = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        inv[SBOX[i] as usize] = i as u8;
        i += 1;
    }
    inv
};

fn expand(key: &[u8; 16]) -> [u8; 176] {
    let mut w = [0u8; 176];
    w[..16].copy_from_slice(key);
    let mut rcon = 1u8;
    for i in 4..44 {
        let mut t = [w[4 * i - 4], w[4 * i - 3], w[4 * i - 2], w[4 * i - 1]];
        if i % 4 == 0 {
            t = [SBOX[t[1] as usize] ^ rcon, SBOX[t[2] as usize], SBOX[t[3] as usize], SBOX[t[0] as usize]];
            rcon = xtime(rcon);
        }
        for j in 0..4 {
            w[4 * i + j] = w[4 * i - 16 + j] ^ t[j];
        }
    }
    w
}

fn add(s: &mut [u8; 16], k: &[u8]) {
    s.iter_mut().zip(k).for_each(|(a, b)| *a ^= b);
}

// state is column-major: s[4 * col + row]
fn shift(s: &mut [u8; 16], inverse: bool) {
    let t = *s;
    for c in 0..4 {
        for r in 0..4 {
            let from = if inverse { (c + 4 - r) % 4 } else { (c + r) % 4 };
            s[4 * c + r] = t[4 * from + r];
        }
    }
}

fn mix(s: &mut [u8; 16], m: [u8; 4]) {
    for c in 0..4 {
        let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
        for r in 0..4 {
            s[4 * c + r] = mul(a[0], m[(4 - r) % 4]) ^ mul(a[1], m[(5 - r) % 4]) ^ mul(a[2], m[(6 - r) % 4])
                ^ mul(a[3], m[(7 - r) % 4]);
        }
    }
}

pub fn encrypt_block(w: &[u8; 176], s: &mut [u8; 16]) {
    add(s, &w[..16]);
    for round in 1..=10 {
        s.iter_mut().for_each(|b| *b = SBOX[*b as usize]);
        shift(s, false);
        if round < 10 {
            mix(s, [2, 3, 1, 1]);
        }
        add(s, &w[16 * round..16 * round + 16]);
    }
}

pub fn decrypt_block(w: &[u8; 176], s: &mut [u8; 16]) {
    add(s, &w[160..]);
    for round in (0..10).rev() {
        shift(s, true);
        s.iter_mut().for_each(|b| *b = INV[*b as usize]);
        add(s, &w[16 * round..16 * round + 16]);
        if round > 0 {
            mix(s, [14, 11, 13, 9]);
        }
    }
}

/// plain ‖ crc32le, PKCS7, CBC. `out` needs `(plain.len() + 4) / 16 * 16 + 16` bytes; returns the length, 0 if
/// `out` is too small.
pub fn seal(key: &[u8; 16], plain: &[u8], out: &mut [u8]) -> usize {
    let n = (plain.len() + 4) / 16 * 16 + 16;
    if out.len() < n {
        return 0;
    }
    let pad = (n - plain.len() - 4) as u8;
    out[..plain.len()].copy_from_slice(plain);
    out[plain.len()..plain.len() + 4].copy_from_slice(&crc32(0, plain).to_le_bytes());
    out[plain.len() + 4..n].fill(pad);
    let w = expand(key);
    let mut prev = IV;
    for chunk in out[..n].chunks_exact_mut(16) {
        let mut s: [u8; 16] = chunk.try_into().unwrap();
        add(&mut s, &prev);
        encrypt_block(&w, &mut s);
        chunk.copy_from_slice(&s);
        prev = s;
    }
    n
}

/// Inverse of `seal`: the plain length in `out`, or None (bad length, padding or CRC).
pub fn open(key: &[u8; 16], cipher: &[u8], out: &mut [u8]) -> Option<usize> {
    if cipher.is_empty() || cipher.len() % 16 != 0 || out.len() < cipher.len() {
        return None;
    }
    let w = expand(key);
    let mut prev = IV;
    for (i, chunk) in cipher.chunks_exact(16).enumerate() {
        let mut s: [u8; 16] = chunk.try_into().unwrap();
        decrypt_block(&w, &mut s);
        add(&mut s, &prev);
        out[16 * i..16 * i + 16].copy_from_slice(&s);
        prev = chunk.try_into().unwrap();
    }
    let pad = usize::from(out[cipher.len() - 1]);
    if !(1..=16).contains(&pad) || out[cipher.len() - pad..cipher.len()].iter().any(|&b| usize::from(b) != pad) {
        return None;
    }
    let body = cipher.len() - pad;
    let n = body.checked_sub(4)?;
    (crc32(0, &out[..n]).to_le_bytes() == out[n..body]).then_some(n)
}

/// K from the pairing secret's 32 ASCII hex chars (NVRAM `APP_SECRET`).
pub fn channel_key(hex: &[u8; 32]) -> Option<[u8; 16]> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut k = [0u8; 16];
    for i in 0..16 {
        k[i] = digit(hex[2 * i])? << 4 | digit(hex[2 * i + 1])?;
    }
    Some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn fips197_c1() {
        let key: [u8; 16] = core::array::from_fn(|i| i as u8);
        let w = expand(&key);
        let mut s: [u8; 16] = hex("00112233445566778899aabbccddeeff").try_into().unwrap();
        encrypt_block(&w, &mut s);
        assert_eq!(s.to_vec(), hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
        decrypt_block(&w, &mut s);
        assert_eq!(s.to_vec(), hex("00112233445566778899aabbccddeeff"));
    }

    #[test]
    fn seal_matches_host() {
        // uv run python -c "from wp2apps import link; print(link.encrypt(bytes([7]*16), b'hello').hex())"
        let mut out = [0u8; 32];
        let n = seal(&[7; 16], b"hello", &mut out);
        assert_eq!(out[..n].to_vec(), hex("73a9c7cf4bb078dd62ad68184de9c20f"));
    }

    #[test]
    fn open_roundtrip_and_rejects() {
        let mut c = [0u8; 160];
        let mut p = [0u8; 160];
        for len in [0usize, 11, 12, 128] {
            let plain: Vec<u8> = (0..len as u8).collect();
            let n = seal(&[3; 16], &plain, &mut c);
            assert_eq!(open(&[3; 16], &c[..n], &mut p), Some(len));
            assert_eq!(&p[..len], &plain[..]);
        }
        let n = seal(&[3; 16], b"hello", &mut c);
        c[0] ^= 1;
        assert_eq!(open(&[3; 16], &c[..n], &mut p), None);
        assert_eq!(open(&[4; 16], &c[..n], &mut p), None);
        assert_eq!(open(&[3; 16], &c[..5], &mut p), None);
    }

    #[test]
    fn key_from_secret() {
        assert_eq!(channel_key(b"00112233445566778899aabbccddeeff").unwrap().to_vec(), hex("00112233445566778899aabbccddeeff"));
        assert_eq!(channel_key(b"0011223344556677889900AABBCCDDEE").unwrap()[15], 0xEE);
        assert!(channel_key(b"zz112233445566778899aabbccddeeff").is_none());
    }
}
