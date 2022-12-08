use sha2::digest::generic_array::{typenum::U32, GenericArray};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct Key(GenericArray<u8, U32>);

impl Key {
    pub fn generate() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"hello world");

        Self(hasher.finalize())
    }
}

#[derive(Clone, Debug)]
pub struct KademliaNode {
    pub id: Key,
}

// construct_uint! {
//     /// 256-bit unsigned integer.
//     pub(super) struct U256(4);
// }

impl KademliaNode {
    pub fn new() -> Self {
        let key = Key::generate();
        Self { id: key }
    }
}
