use std::fs::File;
use std::io::Read;

const K: [u32; 64] = [
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
];
const H0: [u32;8] = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];

#[derive(Clone)]
struct Sha256 { h:[u32;8], tail:[u8;64], tail_len:usize, len:u64 }
impl Sha256 {
    fn new()->Self{Self{h:H0,tail:[0;64],tail_len:0,len:0}}
    fn compress(&mut self,b:&[u8;64]){
        let mut w=[0u32;64];
        for(i,c)in b.chunks_exact(4).enumerate(){w[i]=u32::from_be_bytes([c[0],c[1],c[2],c[3]])}
        for i in 16..64 {
            let s0=w[i-15].rotate_right(7)^w[i-15].rotate_right(18)^(w[i-15]>>3);
            let s1=w[i-2].rotate_right(17)^w[i-2].rotate_right(19)^(w[i-2]>>10);
            w[i]=w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let(mut a,mut b0,mut c,mut d,mut e,mut f,mut g,mut h)=(self.h[0],self.h[1],self.h[2],self.h[3],self.h[4],self.h[5],self.h[6],self.h[7]);
        for i in 0..64{
            let s1=e.rotate_right(6)^e.rotate_right(11)^e.rotate_right(25);
            let ch=(e&f)^(!e&g);
            let t1=h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0=a.rotate_right(2)^a.rotate_right(13)^a.rotate_right(22);
            let maj=(a&b0)^(a&c)^(b0&c);
            let t2=s0.wrapping_add(maj);
            h=g;g=f;f=e;e=d.wrapping_add(t1);d=c;c=b0;b0=a;a=t1.wrapping_add(t2);
        }
        for(i,v)in[a,b0,c,d,e,f,g,h].into_iter().enumerate(){self.h[i]=self.h[i].wrapping_add(v)}
    }
    fn update(&mut self,mut data:&[u8]){
        self.len=self.len.wrapping_add(data.len() as u64);
        if self.tail_len>0{
            let n=(64-self.tail_len).min(data.len());
            self.tail[self.tail_len..self.tail_len+n].copy_from_slice(&data[..n]);
            self.tail_len+=n;data=&data[n..];
            if self.tail_len==64{let block=self.tail;self.compress(&block);self.tail_len=0;}
        }
        while data.len()>=64{let mut block=[0u8;64];block.copy_from_slice(&data[..64]);self.compress(&block);data=&data[64..];}
        self.tail[..data.len()].copy_from_slice(data);self.tail_len=data.len();
    }
    fn finish(mut self)->[u8;32]{
        let bit_len=self.len.wrapping_mul(8);
        self.tail[self.tail_len]=0x80;self.tail_len+=1;
        if self.tail_len>56{self.tail[self.tail_len..].fill(0);let block=self.tail;self.compress(&block);self.tail=[0;64];self.tail_len=0;}
        self.tail[self.tail_len..56].fill(0);self.tail[56..].copy_from_slice(&bit_len.to_be_bytes());let block=self.tail;self.compress(&block);
        let mut out=[0u8;32];for(i,v)in self.h.iter().enumerate(){out[i*4..i*4+4].copy_from_slice(&v.to_be_bytes())}out
    }
}
fn sha256(parts:&[&[u8]])->[u8;32]{let mut s=Sha256::new();for p in parts{s.update(p)}s.finish()}
fn hmac_sha256(key:&[u8],parts:&[&[u8]])->[u8;32]{
    let mut k=[0u8;64];
    if key.len()>64{k[..32].copy_from_slice(&sha256(&[key]));}else{k[..key.len()].copy_from_slice(key);}
    let mut ipad=[0x36u8;64];let mut opad=[0x5cu8;64];
    for i in 0..64{ipad[i]^=k[i];opad[i]^=k[i];}
    let mut inner=Sha256::new();inner.update(&ipad);for p in parts{inner.update(p)}let ih=inner.finish();
    sha256(&[&opad,&ih])
}
pub const ITERATIONS:u32=200_000;
pub fn pbkdf2(password:&[u8],salt:&[u8],iterations:u32)->[u8;32]{
    let block=1u32.to_be_bytes();let mut u=hmac_sha256(password,&[salt,&block]);let mut out=u;
    for _ in 1..iterations{u=hmac_sha256(password,&[&u]);for i in 0..32{out[i]^=u[i];}}
    out
}
pub fn random_salt()->Result<[u8;16],String>{
    let mut salt=[0u8;16];File::open("/dev/urandom").and_then(|mut f|f.read_exact(&mut salt)).map_err(|e|format!("read /dev/urandom: {e}"))?;Ok(salt)
}
pub fn hex(bytes:&[u8])->String{const D:&[u8;16]=b"0123456789abcdef";let mut s=String::with_capacity(bytes.len()*2);for &b in bytes{s.push(D[(b>>4)as usize]as char);s.push(D[(b&15)as usize]as char)}s}
pub fn decode_hex<const N:usize>(s:&str)->Result<[u8;N],String>{
    if s.len()!=N*2{return Err("invalid hex length".into())}let mut out=[0u8;N];let b=s.as_bytes();
    fn nib(v:u8)->Option<u8>{match v{b'0'..=b'9'=>Some(v-b'0'),b'a'..=b'f'=>Some(v-b'a'+10),_=>None}}
    for i in 0..N{out[i]=(nib(b[i*2]).ok_or("invalid hex")?<<4)|nib(b[i*2+1]).ok_or("invalid hex")?;}Ok(out)
}
pub fn constant_time_eq(a:&[u8],b:&[u8])->bool{if a.len()!=b.len(){return false}let mut d=0u8;for(i,x)in a.iter().enumerate(){d|=*x^b[i]}d==0}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn sha_vector(){assert_eq!(hex(&sha256(&[b"abc"])), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");}
    #[test] fn pbkdf2_vector(){assert_eq!(hex(&pbkdf2(b"password",b"salt",1)), "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");}
}
