use core::fmt;
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
    time::Instant,
};

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

    pub fn get(&self, key: &Key) -> Option<Cow<'_, Record>> {
        self.records.get(key).map(Cow::Borrowed)
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
    #[serde(with = "serde_millis")]
    pub expires: Option<Instant>,
}

impl Record {
    pub fn new<K: Into<Key>>(key: K, value: Vec<u8>) -> Self {
        Record {
            key: key.into(),
            value,
            publisher: None,
            expires: None,
        }
    }

    pub fn set_publisher<K: Into<Key>>(&mut self, key: K) -> &mut Self {
        self.publisher = Some(key.into());
        self
    }

    pub fn set_expiration(&mut self, expires: Instant) -> &mut Self {
        self.expires = Some(expires);
        self
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        self.expires.map_or(false, |t| now >= t)
    }
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = String::from_utf8(self.value.clone()).unwrap();
        write!(f, "{{ key: {}, value: {} }}", self.key, value)
    }
}
