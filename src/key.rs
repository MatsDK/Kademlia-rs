use core_::borrow::Borrow;
use rand::Rng;
use sha2::{
    digest::{generic_array::GenericArray, typenum::U32},
    Digest, Sha256,
};
use uint::*;

#[derive(Clone, Debug)]
pub struct Key(GenericArray<u8, U32>);

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
        Key(Sha256::digest(value.borrow()))
    }

    pub fn random() -> Self {
        let id = rand::thread_rng().gen::<[u8; 32]>();
        let bytes = Sha256::digest(id);

        Self(bytes)
    }

    pub fn distance(&self, other: &Key) -> Distance {
        let a = U256::from(self.0.as_slice());
        let b = U256::from(other.0.as_slice());

        Distance(a ^ b)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Distance(pub(super) U256);

impl Distance {
    pub fn diff_bits(&self) -> Option<u32> {
        (256 - self.0.leading_zeros()).checked_sub(1)
    }
}
