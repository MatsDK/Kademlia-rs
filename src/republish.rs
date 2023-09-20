use std::{
    task::{Context, Poll},
    time::{Duration, Instant},
    vec,
};

use crate::{
    key::Key,
    store::{Record, RecordStore},
};

#[derive(Debug)]
pub struct RepublishJob {
    local_key: Key,
    record_ttl: Option<Duration>,
    republish_interval: Option<Duration>,
    replication_interval: Duration,
    next_publish: Option<Instant>,
    state: JobState,
}

#[derive(Debug)]
enum JobState {
    Running(vec::IntoIter<Record>),
    Waiting(Instant),
}

impl RepublishJob {
    pub fn new(
        local_key: Key,
        record_ttl: Option<Duration>,
        replication_interval: Duration,
        republish_interval: Option<Duration>,
    ) -> Self {
        let now = Instant::now();
        let next_publish = republish_interval.map(|i| now + i);

        Self {
            local_key,
            record_ttl,
            republish_interval,
            replication_interval,
            next_publish,
            state: JobState::Waiting(now + replication_interval),
        }
    }

    pub fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        store: &mut RecordStore,
        now: Instant,
    ) -> Poll<Record> {
        // println!("Poll replicate job");
        if let JobState::Waiting(deadline) = self.state {
            if now >= deadline {
                let should_publish = self.next_publish.map_or(false, |i| now >= i);
                // println!("Run replicate cycle, should publish this cycle: {should_publish}");

                let records = store
                    .all_records()
                    .iter()
                    .filter_map(|&record| {
                        let is_publisher = record.publisher == Some(self.local_key);

                        // The original publisher of a record is not responible for re-publising
                        if is_publisher && !should_publish {
                            None
                        } else {
                            let mut record = record.clone();
                            if should_publish && is_publisher {
                                record.expires = record
                                    .expires
                                    .or_else(|| self.record_ttl.map(|ttl| now + ttl));
                            }
                            Some(record.clone())
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_iter();

                // If this iteration is republishing records, set the next republish deadline.
                if should_publish {
                    self.next_publish = self.republish_interval.map(|i| now + i);
                }

                self.state = JobState::Running(records);
            }
        }

        if let JobState::Running(records) = &mut self.state {
            // println!("Running the replication/republishing, records: {records:?}");
            for record in records {
                if record.is_expired(now) {
                    store.remove(&record.key)
                } else {
                    return Poll::Ready(record);
                }
            }

            // println!("done running");

            // After all records are republished/replicated, reset the state to waiting.
            let next_deadline = now + self.replication_interval;
            self.state = JobState::Waiting(next_deadline);
        }

        Poll::Pending
    }
}
