//! FHashedName: how the cooked shader maps name shader types, vertex factory types, pipelines and uniform buffer
//! structs. The value is CUE4Parse 1.2.2's HashedNamesProvider: CityHash64WithSeed of
//! the upper-cased UTF-8 name, the seed 0; a name ending in `_<n>` (an FName number, no leading zero) hashes the part
//! before it with the seed n + 1.

const K0: u64 = 0xc3a5c85c97cb3127;
const K1: u64 = 0xb492b66fbe98f273;
const K2: u64 = 0x9ae16a3b2f90404f;
const KMUL: u64 = 0x9ddfea08eb382d69;

fn f64le(s: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(s[i..i + 8].try_into().unwrap())
}

fn f32le(s: &[u8], i: usize) -> u64 {
    u32::from_le_bytes(s[i..i + 4].try_into().unwrap()) as u64
}

fn shift_mix(v: u64) -> u64 {
    v ^ (v >> 47)
}

fn hash128to64(u: u64, v: u64) -> u64 {
    hash_len16(u, v, KMUL)
}

fn hash_len16(u: u64, v: u64, m: u64) -> u64 {
    let mut a = (u ^ v).wrapping_mul(m);
    a ^= a >> 47;
    let mut b = (v ^ a).wrapping_mul(m);
    b ^= b >> 47;
    b.wrapping_mul(m)
}

fn hash_len0to16(s: &[u8]) -> u64 {
    let len = s.len();
    if len >= 8 {
        let m = K2.wrapping_add(len as u64 * 2);
        let a = f64le(s, 0).wrapping_add(K2);
        let b = f64le(s, len - 8);
        let c = b.rotate_right(37).wrapping_mul(m).wrapping_add(a);
        let d = a.rotate_right(25).wrapping_add(b).wrapping_mul(m);
        return hash_len16(c, d, m);
    }
    if len >= 4 {
        let m = K2.wrapping_add(len as u64 * 2);
        let a = f32le(s, 0);
        return hash_len16((len as u64).wrapping_add(a << 3), f32le(s, len - 4), m);
    }
    if len > 0 {
        let y = s[0] as u32 + ((s[len >> 1] as u32) << 8);
        let z = len as u32 + ((s[len - 1] as u32) << 2);
        return shift_mix((y as u64).wrapping_mul(K2) ^ (z as u64).wrapping_mul(K0)).wrapping_mul(K2);
    }
    K2
}

fn hash_len17to32(s: &[u8]) -> u64 {
    let len = s.len();
    let m = K2.wrapping_add(len as u64 * 2);
    let a = f64le(s, 0).wrapping_mul(K1);
    let b = f64le(s, 8);
    let c = f64le(s, len - 8).wrapping_mul(m);
    let d = f64le(s, len - 16).wrapping_mul(K2);
    hash_len16(
        a.wrapping_add(b).rotate_right(43).wrapping_add(c.rotate_right(30)).wrapping_add(d),
        a.wrapping_add(b.wrapping_add(K2).rotate_right(18)).wrapping_add(c),
        m,
    )
}

fn hash_len33to64(s: &[u8]) -> u64 {
    let len = s.len();
    let m = K2.wrapping_add(len as u64 * 2);
    let mut a = f64le(s, 0).wrapping_mul(K2);
    let mut b = f64le(s, 8);
    let c = f64le(s, len - 24);
    let d = f64le(s, len - 32);
    let e = f64le(s, 16).wrapping_mul(K2);
    let f = f64le(s, 24).wrapping_mul(9);
    let g = f64le(s, len - 8);
    let h = f64le(s, len - 16).wrapping_mul(m);
    let u = a.wrapping_add(g).rotate_right(43).wrapping_add(b.rotate_right(30).wrapping_add(c).wrapping_mul(9));
    let v = (a.wrapping_add(g) ^ d).wrapping_add(f).wrapping_add(1);
    let w = u.wrapping_add(v).wrapping_mul(m).swap_bytes().wrapping_add(h);
    let x = e.wrapping_add(f).rotate_right(42).wrapping_add(c);
    let y = v.wrapping_add(w).wrapping_mul(m).swap_bytes().wrapping_add(g).wrapping_mul(m);
    let z = e.wrapping_add(f).wrapping_add(c);
    a = x.wrapping_add(z).wrapping_mul(m).wrapping_add(y).swap_bytes().wrapping_add(b);
    b = shift_mix(z.wrapping_add(a).wrapping_mul(m).wrapping_add(d).wrapping_add(h)).wrapping_mul(m);
    b.wrapping_add(x)
}

fn weak(w: u64, x: u64, y: u64, z: u64, mut a: u64, mut b: u64) -> (u64, u64) {
    a = a.wrapping_add(w);
    b = b.wrapping_add(a).wrapping_add(z).rotate_right(21);
    let c = a;
    a = a.wrapping_add(x).wrapping_add(y);
    b = b.wrapping_add(a.rotate_right(44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

fn weak_at(s: &[u8], i: usize, a: u64, b: u64) -> (u64, u64) {
    weak(f64le(s, i), f64le(s, i + 8), f64le(s, i + 16), f64le(s, i + 24), a, b)
}

/// CityHash64 (v1.1), as CUE4Parse 1.2.2's CityHash.
pub fn city_hash64(s: &[u8]) -> u64 {
    let len = s.len();
    if len <= 16 {
        return hash_len0to16(s);
    }
    if len <= 32 {
        return hash_len17to32(s);
    }
    if len <= 64 {
        return hash_len33to64(s);
    }
    let mut x = f64le(s, len - 40);
    let mut y = f64le(s, len - 16).wrapping_add(f64le(s, len - 56));
    let mut z = hash128to64(f64le(s, len - 48).wrapping_add(len as u64), f64le(s, len - 24));
    let mut v = weak_at(s, len - 64, len as u64, z);
    let mut w = weak_at(s, len - 32, y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(f64le(s, 0));
    let mut left = (len - 1) & !63;
    let mut p = 0;
    loop {
        x = x.wrapping_add(y).wrapping_add(v.0).wrapping_add(f64le(s, p + 8)).rotate_right(37).wrapping_mul(K1);
        y = y.wrapping_add(v.1).wrapping_add(f64le(s, p + 48)).rotate_right(42).wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(f64le(s, p + 40));
        z = z.wrapping_add(w.0).rotate_right(33).wrapping_mul(K1);
        v = weak_at(s, p, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak_at(s, p + 32, z.wrapping_add(w.1), y.wrapping_add(f64le(s, p + 16)));
        std::mem::swap(&mut z, &mut x);
        p += 64;
        left -= 64;
        if left == 0 {
            break;
        }
    }
    hash128to64(
        hash128to64(v.0, w.0).wrapping_add(shift_mix(y).wrapping_mul(K1)).wrapping_add(z),
        hash128to64(v.1, w.1).wrapping_add(x),
    )
}

/// The FHashedName of a name (a shader type, vertex factory, pipeline or uniform buffer struct name).
pub fn hashed_name(name: &str) -> u64 {
    let (mut text, mut seed) = (name, 0u64);
    if let Some(us) = name.rfind('_') {
        let tail = &name[us + 1..];
        // an FName number has no leading zero (`_01` is part of the name)
        if us > 0
            && !tail.is_empty()
            && tail.bytes().all(|b| b.is_ascii_digit())
            && (tail.len() == 1 || !tail.starts_with('0'))
            && let Ok(n) = tail.parse::<u32>()
            && n <= 0x7fff_ffff
        {
            text = &name[..us];
            seed = n as u64 + 1;
        }
    }
    let h = city_hash64(text.to_uppercase().as_bytes());
    // CityHash64WithSeeds(s, k2, seed): HashLen16(CityHash64(s) - k2, seed)
    hash128to64(h.wrapping_sub(K2), seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values the importer's shader hash (cityHash64 and hashedName) computes for the same inputs.
    #[test]
    fn matches_the_ts() {
        let text = b"abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz";
        let city: [(usize, u64); 18] = [
            (0, 0x9ae16a3b2f90404f),
            (1, 0xb3454265b6df75e3),
            (3, 0x24a5b3a074e7f369),
            (4, 0x1a5502de4a1f8101),
            (7, 0x3c40c92b1ccb7355),
            (8, 0xfee9d22990c82909),
            (15, 0x687dfc2653bb2c1d),
            (16, 0x54630bad7705cebd),
            (17, 0x9e2106c72d7793a8),
            (31, 0x6e388c9af6bb4dc),
            (32, 0xcf7d548b260f41a),
            (33, 0xc28e9ca56b951ebf),
            (63, 0x646fd5e19b0f36aa),
            (64, 0x2f9491c03daab9f0),
            (65, 0xd5ac6bb8bfd60202),
            (127, 0x3db7771daa910c83),
            (128, 0x37f994cec212325a),
            (129, 0xa133d96108cc422f),
        ];
        for (len, want) in city {
            assert_eq!(city_hash64(&text[..len]), want, "cityHash64 of {len} bytes");
        }
        let names: [(&str, u64); 17] = [
            ("", 0x0000000000000000),
            ("a", 0x35552DC3742BA4AD),
            ("ab", 0xAB20A1FE2592665D),
            ("abc", 0x26B6262FC3CFBA62),
            ("View", 0xF10CD51552E37C76),
            ("Material", 0x464D5173DC80B663),
            ("FTonemapPS", 0x08C44CA61B19724D),
            ("FLocalVertexFactory", 0x9F41D10D68BB52D8),
            ("TBasePassPSFNoLightMapPolicy", 0x4507F03A199C6526),
            ("TGPUSkinVertexFactoryDefault", 0x391C2B78C893A52A),
            ("TBasePassPSFPrecomputedVolumetricLightmapLightingPolicySkylight", 0x18A82EBDE5D3D122),
            ("TSlateMaterialShaderPSDefault_0", 0x9C5084960289E0EC),
            ("MaterialCollection_1", 0x2EF0905BB84F02DD),
            ("Name_01", 0xBBCE369506F76B43),
            ("X_2147483647", 0xBBCAE754DBBA63A3),
            ("_5", 0x1914BCCF8D414278),
            (
                "TBasePassPSFPrecomputedVolumetricLightmapLightingPolicySkylight_and_some_more_text_to_pass_64_bytes",
                0xEE09B55A7F742E78,
            ),
        ];
        for (name, want) in names {
            assert_eq!(hashed_name(name), want, "hashedName({name:?})");
        }
    }
}
