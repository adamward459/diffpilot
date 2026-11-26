//! CPython's `random.Random` (MT19937) for integer seeds that fit in 32 bits, so the fixture repo
//! keeps the exact files the Python script made and the committed screenshots and baselines stay valid.

const N: usize = 624;
const M: usize = 397;

pub struct PyRandom {
    mt: [u32; N],
    index: usize,
}

impl PyRandom {
    pub fn new(seed: u32) -> Self {
        let mut rng = Self { mt: [0; N], index: N };
        rng.init_genrand(19_650_218);
        let key = [seed];
        let (mut i, mut j) = (1, 0);
        for _ in 0..N.max(key.len()) {
            let prev = rng.mt[i - 1] ^ (rng.mt[i - 1] >> 30);
            rng.mt[i] = (rng.mt[i] ^ prev.wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                rng.mt[0] = rng.mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            let prev = rng.mt[i - 1] ^ (rng.mt[i - 1] >> 30);
            rng.mt[i] = (rng.mt[i] ^ prev.wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                rng.mt[0] = rng.mt[N - 1];
                i = 1;
            }
        }
        rng.mt[0] = 0x8000_0000;
        rng
    }

    fn init_genrand(&mut self, seed: u32) {
        self.mt[0] = seed;
        for i in 1..N {
            let prev = self.mt[i - 1] ^ (self.mt[i - 1] >> 30);
            self.mt[i] = prev.wrapping_mul(1_812_433_253).wrapping_add(i as u32);
        }
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            for k in 0..N {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % N] & 0x7fff_ffff);
                let mag = if y & 1 == 1 { 0x9908_b0df } else { 0 };
                self.mt[k] = self.mt[(k + M) % N] ^ (y >> 1) ^ mag;
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    fn getrandbits(&mut self, k: u32) -> u32 {
        self.next_u32() >> (32 - k)
    }

    pub fn randrange(&mut self, n: u32) -> u32 {
        let k = u32::BITS - n.leading_zeros();
        loop {
            let r = self.getrandbits(k);
            if r < n {
                return r;
            }
        }
    }

    pub fn choice<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.randrange(items.len() as u32) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::PyRandom;

    #[test]
    fn matches_cpython_for_the_fixture_seed() {
        let mut rng = PyRandom::new(0x9E37_79B9);
        let hundreds: Vec<u32> = (0..8).map(|_| rng.randrange(100)).collect();
        let fives: Vec<u32> = (0..4).map(|_| rng.randrange(5)).collect();
        let picked = rng.randrange(11);
        assert_eq!(hundreds, [44, 50, 81, 30, 53, 84, 79, 22]);
        assert_eq!(fives, [1, 3, 0, 2]);
        assert_eq!(picked, 3);
        assert_eq!([rng.next_u32(), rng.next_u32()], [782_773_163, 448_621_727]);
    }
}
