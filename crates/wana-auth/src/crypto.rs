use std::fs::File;
use std::io::Read;

const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

struct Sha256 {
    h: [u32; 8],
    tail: [u8; 64],
    tail_len: usize,
    len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            h: H0,
            tail: [0; 64],
            tail_len: 0,
            len: 0,
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (
            self.h[0],
            self.h[1],
            self.h[2],
            self.h[3],
            self.h[4],
            self.h[5],
            self.h[6],
            self.h[7],
        );

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (index, value) in [a, b, c, d, e, f, g, h].into_iter().enumerate() {
            self.h[index] = self.h[index].wrapping_add(value);
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);

        if self.tail_len > 0 {
            let count = (64 - self.tail_len).min(data.len());
            self.tail[self.tail_len..self.tail_len + count].copy_from_slice(&data[..count]);
            self.tail_len += count;
            data = &data[count..];

            if self.tail_len == 64 {
                let block = self.tail;
                self.compress(&block);
                self.tail_len = 0;
            }
        }

        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }

        self.tail[..data.len()].copy_from_slice(data);
        self.tail_len = data.len();
    }

    fn finish(mut self) -> [u8; 32] {
        let bit_len = self.len.wrapping_mul(8);
        self.tail[self.tail_len] = 0x80;
        self.tail_len += 1;

        if self.tail_len > 56 {
            self.tail[self.tail_len..].fill(0);
            let block = self.tail;
            self.compress(&block);
            self.tail = [0; 64];
            self.tail_len = 0;
        }

        self.tail[self.tail_len..56].fill(0);
        self.tail[56..].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.tail;
        self.compress(&block);

        let mut out = [0u8; 32];
        for (index, value) in self.h.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        out
    }
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut state = Sha256::new();
    for part in parts {
        state.update(part);
    }
    state.finish()
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut normalized = [0u8; 64];
    if key.len() > normalized.len() {
        normalized[..32].copy_from_slice(&sha256(&[key]));
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for index in 0..64 {
        inner_pad[index] ^= normalized[index];
        outer_pad[index] ^= normalized[index];
    }

    let mut inner = Sha256::new();
    inner.update(&inner_pad);
    for part in parts {
        inner.update(part);
    }
    let inner_hash = inner.finish();

    sha256(&[&outer_pad, &inner_hash])
}

pub const ITERATIONS: u32 = 200_000;

pub fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let block_index = 1u32.to_be_bytes();
    let mut u = hmac_sha256(password, &[salt, &block_index]);
    let mut out = u;

    for _ in 1..iterations {
        u = hmac_sha256(password, &[&u]);
        for index in 0..out.len() {
            out[index] ^= u[index];
        }
    }

    out
}

pub fn random_salt() -> Result<[u8; 16], String> {
    let mut salt = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut salt))
        .map_err(|e| format!("read /dev/urandom: {e}"))?;
    Ok(salt)
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], String> {
    if text.len() != N * 2 {
        return Err("invalid hex length".into());
    }

    fn nibble(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        }
    }

    let bytes = text.as_bytes();
    let mut out = [0u8; N];
    for index in 0..N {
        let high = nibble(bytes[index * 2]).ok_or("invalid hex")?;
        let low = nibble(bytes[index * 2 + 1]).ok_or("invalid hex")?;
        out[index] = (high << 4) | low;
    }
    Ok(out)
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (index, value) in a.iter().enumerate() {
        difference |= *value ^ b[index];
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_vector() {
        assert_eq!(
            hex(&sha256(&[b"abc"])),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn pbkdf2_vector() {
        assert_eq!(
            hex(&pbkdf2(b"password", b"salt", 1)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }
}
