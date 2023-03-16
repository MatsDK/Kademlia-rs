use core_::array::TryFromSliceError;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{borrow::Borrow, fmt, str::FromStr};
use uint::*;

// #[derive(Clone, Debug)]
// pub struct Key(GenericArray<u8, U32>);
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct Key([u8; 32]);

construct_uint! {
    /// 256-bit unsigned integer.
    pub(super) struct U256(4);
}

impl Key {
    #[allow(dead_code)]
    pub fn new<T>(value: T) -> Self
    where
        T: Borrow<[u8]>,
    {
        Key(Sha256::digest(value.borrow()).into())
    }

    pub fn random() -> Self {
        let id = rand::thread_rng().gen::<[u8; 32]>();
        let bytes = Sha256::digest(id);

        Self(bytes.into())
    }

    pub fn distance(&self, other: &Key) -> Distance {
        let a = U256::from(self.0.as_slice());
        let b = U256::from(other.0.as_slice());

        Distance(a ^ b)
    }
}

impl FromStr for Key {
    type Err = TryFromSliceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded = bs58::decode(s).into_vec().unwrap();
        <&[u8] as TryInto<[u8; 32]>>::try_into(&decoded[..32]).map(Key)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Distance(pub(super) U256);

impl Distance {
    pub fn ilog2(&self) -> Option<u32> {
        (256 - self.0.leading_zeros()).checked_sub(1)
    }

    pub fn leading_zeros(&self) -> Option<u32> {
        let leading_zeros = self.0.leading_zeros();

        // distance should not be 0
        if leading_zeros == 256 {
            return None;
        }

        Some(leading_zeros)
    }
}
