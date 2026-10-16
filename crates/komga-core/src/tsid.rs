//! TSID256: compatible with `TsidCreator.getTsid256()` from com.github.f4b6a3:tsid-creator 5.2.6.
//! 64-bit layout: 42bit milliseconds (epoch 2020-01-01T00:00:00Z) | 8bit node | 14bit counter.
//! The string form is 13-character uppercase Crockford Base32, whose lexicographic order matches time order.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const TSID_EPOCH_MILLIS: u64 = 1_577_836_800_000; // 2020-01-01T00:00:00Z
const COUNTER_BITS: u64 = 14;
const COUNTER_MASK: u64 = (1 << COUNTER_BITS) - 1;
const NODE_BITS: u64 = 8;
const NODE_MASK: u64 = (1 << NODE_BITS) - 1;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tsid(u64);

impl Tsid {
    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// 13-character Crockford Base32 (uppercase, no I/L/O/U).
    pub fn encode(self) -> String {
        let mut chars = [0u8; 13];
        for (i, ch) in chars.iter_mut().enumerate() {
            let shift = (12 - i) * 5;
            *ch = ALPHABET[((self.0 >> shift) & 0b11111) as usize];
        }
        // Safe: the alphabet is all ASCII
        String::from_utf8(chars.to_vec()).unwrap()
    }

    pub fn parse(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() != 13 {
            return None;
        }
        let mut value: u64 = 0;
        for &b in bytes {
            let digit = ALPHABET.iter().position(|&a| a == b)? as u64;
            value = (value << 5) | digit;
        }
        Some(Self(value))
    }
}

impl std::fmt::Display for Tsid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.encode())
    }
}

/// A single instance per process is enough; node defaults to a random 8bit value and the counter starts at a random value, matching tsid-creator.
pub struct TsidFactory {
    node: u64,
    state: Mutex<State>,
}

struct State {
    last_ms: u64,
    counter: u64,
}

impl TsidFactory {
    pub fn new(node: u16) -> Self {
        Self {
            node: (node as u64) & NODE_MASK,
            state: Mutex::new(State {
                last_ms: 0,
                counter: rand_u64() & COUNTER_MASK,
            }),
        }
    }

    /// Uses a random node value (tsid-creator's default behavior).
    pub fn new_random_node() -> Self {
        Self::new((rand_u64() & NODE_MASK) as u16)
    }

    pub fn create(&self) -> Tsid {
        let mut state = self.state.lock().unwrap();
        let mut ms = now_millis().saturating_sub(TSID_EPOCH_MILLIS);
        if ms <= state.last_ms {
            ms = state.last_ms;
            state.counter = (state.counter + 1) & COUNTER_MASK;
            if state.counter == 0 {
                // Counter wrapped: advance to the next millisecond (spin-wait for the real clock)
                ms += 1;
                while now_millis().saturating_sub(TSID_EPOCH_MILLIS) < ms {
                    std::hint::spin_loop();
                }
            }
        }
        state.last_ms = ms;
        let value =
            (ms << (NODE_BITS + COUNTER_BITS)) | (self.node << COUNTER_BITS) | state.counter;
        Tsid(value)
    }

    pub fn create_string(&self) -> String {
        self.create().encode()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn rand_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    RandomState::new().build_hasher().finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_roundtrip() {
        let tsid = Tsid::from_u64(0x0F00_DEAD_BEEF_CAFE);
        let s = tsid.to_string();
        assert_eq!(s.len(), 13);
        assert_eq!(Tsid::parse(&s), Some(tsid));
    }

    #[test]
    fn known_value() {
        // Hand-computed: value=0 → "0000000000000"; value=1 → "0000000000001"; value=32 → "0000000000010"
        assert_eq!(Tsid::from_u64(0).to_string(), "0000000000000");
        assert_eq!(Tsid::from_u64(1).to_string(), "0000000000001");
        assert_eq!(Tsid::from_u64(32).to_string(), "0000000000010");
        assert_eq!(Tsid::from_u64(31).to_string(), "000000000000Z");
    }

    #[test]
    fn monotonic_within_same_ms() {
        let factory = TsidFactory::new(1);
        let ids: Vec<Tsid> = (0..1000).map(|_| factory.create()).collect();
        for w in ids.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn counter_wrap_does_not_regress() {
        let factory = TsidFactory::new(1);
        let ids: Vec<Tsid> = (0..20_000).map(|_| factory.create()).collect();
        for w in ids.windows(2) {
            assert!(w[0] < w[1]);
        }
    }
}
