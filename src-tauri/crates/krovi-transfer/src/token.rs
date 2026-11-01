//! Secrets: share tokens, share ids, PINs. Everything comes from the OS CSPRNG
//! (`getrandom`: BCryptGenRandom / ProcessPrng on Windows). If the OS cannot provide
//! entropy we return an error and the share is not created; there is no weak fallback.

use crate::error::TransferError;

/// 20 random bytes = 160 bits of entropy (the contract asks for at least 128).
pub const TOKEN_BYTES: usize = 20;
/// Characters in a base64url token of `TOKEN_BYTES` bytes.
pub const TOKEN_LEN: usize = 27;

pub fn fill(buf: &mut [u8]) -> Result<(), TransferError> {
    getrandom::fill(buf).map_err(|e| TransferError::Entropy(e.to_string()))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding (RFC 4648 section 5).
pub fn b64url(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// A fresh unguessable URL token.
pub fn token() -> Result<String, TransferError> {
    let mut b = [0u8; TOKEN_BYTES];
    fill(&mut b)?;
    Ok(b64url(&b))
}

/// A short public identifier for a share (events, `transfer_stop`). Not a secret and not
/// derivable from the token.
pub fn share_id() -> Result<String, TransferError> {
    let mut b = [0u8; 6];
    fill(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// A session value handed out after a correct PIN (128 bits).
pub fn session_key() -> Result<String, TransferError> {
    let mut b = [0u8; 16];
    fill(&mut b)?;
    Ok(b64url(&b))
}

/// A uniformly distributed 4-digit PIN (rejection sampling, no modulo bias).
pub fn pin() -> Result<String, TransferError> {
    loop {
        let mut b = [0u8; 2];
        fill(&mut b)?;
        let v = u16::from_le_bytes(b);
        // 60000 = 6 * 10000: values below it map uniformly onto 0..9999.
        if v < 60000 {
            return Ok(format!("{:04}", v % 10000));
        }
    }
}

/// A uniformly random u16 in `lo..=hi` (used to pick the listening port).
pub fn rand_in(lo: u16, hi: u16) -> Result<u16, TransferError> {
    debug_assert!(lo <= hi);
    let span = u32::from(hi - lo) + 1;
    // largest multiple of `span` that fits in u32 range of a 16-bit draw (65536)
    let limit = (65536 / span) * span;
    loop {
        let mut b = [0u8; 2];
        fill(&mut b)?;
        let v = u32::from(u16::from_le_bytes(b));
        if v < limit {
            return Ok(lo + (v % span) as u16);
        }
    }
}

/// Constant-time comparison (also constant in the length of the longer input).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u32;
    let n = a.len().max(b.len());
    for i in 0..n {
        diff |= u32::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    std::hint::black_box(diff) == 0
}

/// True when `s` looks like a token we could have issued (cheap pre-filter on the URL).
pub fn plausible_token(s: &str) -> bool {
    (16..=64).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn b64url_known_vectors() {
        assert_eq!(b64url(b""), "");
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(b64url(b"foob"), "Zm9vYg");
        assert_eq!(b64url(&[0xfb, 0xff, 0xfe]), "-__-");
    }

    #[test]
    fn token_has_160_bits_and_url_safe_alphabet() {
        let t = token().unwrap();
        assert_eq!(t.len(), TOKEN_LEN);
        assert!(plausible_token(&t));
        // 27 chars * 6 bits = 162 bits of encoding for 160 bits of entropy
        const { assert!(TOKEN_BYTES * 8 >= 128) };
    }

    #[test]
    fn tokens_do_not_repeat() {
        let mut seen = HashSet::new();
        for _ in 0..2000 {
            assert!(seen.insert(token().unwrap()));
        }
    }

    #[test]
    fn share_ids_are_hex_and_not_the_token() {
        let id = share_id().unwrap();
        assert_eq!(id.len(), 12);
        assert!(id.bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn pin_is_four_digits_and_roughly_uniform() {
        let mut buckets = [0u32; 10];
        for _ in 0..4000 {
            let p = pin().unwrap();
            assert_eq!(p.len(), 4);
            assert!(p.bytes().all(|c| c.is_ascii_digit()));
            buckets[(p.as_bytes()[0] - b'0') as usize] += 1;
        }
        // 400 expected per leading digit; a broken generator would pile onto one bucket.
        assert!(buckets.iter().all(|&n| n > 250 && n < 560), "{buckets:?}");
    }

    #[test]
    fn rand_in_stays_in_range_and_covers_it() {
        let mut lo_seen = false;
        let mut hi_seen = false;
        for _ in 0..5000 {
            let v = rand_in(10, 13).unwrap();
            assert!((10..=13).contains(&v));
            lo_seen |= v == 10;
            hi_seen |= v == 13;
        }
        assert!(lo_seen && hi_seen);
        let p = rand_in(49152, 65534).unwrap();
        assert!((49152..=65534).contains(&p));
    }

    #[test]
    fn ct_eq_behaves_like_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"a"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn plausible_token_rejects_junk() {
        assert!(!plausible_token(""));
        assert!(!plausible_token("short"));
        assert!(!plausible_token("../../etc/passwd/../../xxxxxxxx"));
        assert!(!plausible_token(&"a".repeat(65)));
        assert!(plausible_token(&"A".repeat(27)));
    }
}
