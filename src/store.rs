use core::fmt;
use std::collections::{hash_map::Entry, HashMap};

use serde::{Deserialize, Serialize};

use crate::key::Key;

#[derive(Debug)]
#[allow(unused)]
pub struct RecordStore {
    local_key: Key,
    records: HashMap<Key, Record>,
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

    pub fn get(&self, key: &Key) -> Option<&Record> {
        self.records.get(key)
    }

    pub fn put(&mut self, record: Record) -> Result<(), ()> {
        match self.records.entry(record.key.clone()) {
            Entry::Occupied(mut e) => {
                e.insert(record);
            }
            Entry::Vacant(e) => {
                // if num_records >= self.config.max_records {
                //     return Err(Error::MaxRecords);
                // }
                e.insert(record);
            }
        }

        Ok(())
    }

    pub fn remove(&mut self, key: &Key) {
        self.records.remove(key);
    }

    // #[cfg(feature = "debug")]
    pub fn get_all_records(&self) -> Vec<&Record> {
        self.records
            .iter()
            .map(|(_, record)| record)
            .collect::<Vec<_>>()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Record {
    pub key: Key,
    pub value: Vec<u8>,
    pub publisher: Option<Key>,
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = String::from_utf8(self.value.clone()).unwrap();
        write!(f, "{{ key: {}, value: {} }}", self.key, value)
    }
}
