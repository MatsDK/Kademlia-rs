use std::collections::HashMap;

use crate::key::Key;

#[derive(Debug)]
pub struct RecordStore {
    local_key: Key,
    records: HashMap<Key, ()>,
    providers: HashMap<Key, Vec<Key>>,
}

impl RecordStore {
    pub fn new(local_key: Key) -> Self {
        Self {
            local_key,
            records: HashMap::default(),
            providers: HashMap::default(),
        }
    }

    fn get(&self) {}

    fn put(&mut self) {}

    fn remove(&mut self) {}
}
