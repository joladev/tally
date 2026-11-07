use std::path::Path;

use fjall::{Database, Keyspace, KeyspaceCreateOptions, PersistMode};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan as _};
use serde::Serialize;

use crate::counts::Counts;
use crate::zones::Zone;

const SECONDS_PER_HOUR: i64 = 3_600;
const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Hour,
    Day,
    Month,
}

impl Period {
    pub fn start(self, time: i64) -> i64 {
        match self {
            Period::Hour => time - time.rem_euclid(SECONDS_PER_HOUR),
            Period::Day => time - time.rem_euclid(SECONDS_PER_DAY),
            Period::Month => seconds(date(time).first_of_month()),
        }
    }

    pub fn next(self, start: i64) -> i64 {
        match self {
            Period::Hour => start + SECONDS_PER_HOUR,
            Period::Day => start + SECONDS_PER_DAY,
            Period::Month => {
                let next = date(start).checked_add(1.month());
                seconds(next.expect("months after a valid time are valid"))
            }
        }
    }
}

pub struct Store {
    database: Database,
    counts: Keyspace,
    zones: Keyspace,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Store> {
        let database = Database::builder(path).open()?;
        let counts = database.keyspace("counts", KeyspaceCreateOptions::default)?;
        let zones = database.keyspace("zones", KeyspaceCreateOptions::default)?;
        Ok(Store {
            database,
            counts,
            zones,
        })
    }

    pub fn add(&self, zone: u64, hour: i64, counts: &Counts) -> anyhow::Result<()> {
        let mut batch = self.database.batch();
        for period in [Period::Hour, Period::Day, Period::Month] {
            let key = key(zone, period, period.start(hour));
            let mut row = match self.counts.get(key)? {
                Some(bytes) => postcard::from_bytes(&bytes)?,
                None => Counts::default(),
            };
            row.merge(counts);
            batch.insert(&self.counts, key, postcard::to_stdvec(&row)?);
        }
        Ok(batch.commit()?)
    }

    pub fn rows(&self, zone: u64, period: Period, from: i64) -> anyhow::Result<Vec<(i64, Counts)>> {
        let range = key(zone, period, from)..=key(zone, period, i64::MAX);
        self.counts
            .range(range)
            .map(|entry| {
                let (key, value) = entry.into_inner()?;
                let start = i64::from_be_bytes(key[9..].try_into()?);
                Ok((start, postcard::from_bytes(&value)?))
            })
            .collect()
    }

    pub fn zones_saved(&self) -> anyhow::Result<Vec<Zone>> {
        self.zones
            .iter()
            .map(|entry| Ok(serde_json::from_slice(&entry.value()?)?))
            .collect()
    }

    pub fn save_zone(&self, zone: &Zone) -> anyhow::Result<()> {
        self.zones
            .insert(zone.id.to_be_bytes(), serde_json::to_vec(zone)?)?;
        self.persist()
    }

    pub fn persist(&self) -> anyhow::Result<()> {
        Ok(self.database.persist(PersistMode::SyncData)?)
    }
}

fn key(zone: u64, period: Period, start: i64) -> [u8; 17] {
    let mut key = [0; 17];
    key[..8].copy_from_slice(&zone.to_be_bytes());
    key[8] = period as u8;
    key[9..].copy_from_slice(&start.to_be_bytes());
    key
}

fn date(time: i64) -> Date {
    let timestamp = Timestamp::from_second(time).expect("only valid times are stored");
    timestamp.to_zoned(TimeZone::UTC).date()
}

fn seconds(date: Date) -> i64 {
    let zoned = date
        .to_zoned(TimeZone::UTC)
        .expect("a valid date starts at a valid time");
    zoned.timestamp().as_second()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATE_MARCH: i64 = 1_711_927_800;
    const APRIL: i64 = 1_711_929_600;

    #[test]
    fn periods_start_and_end_on_utc_boundaries() {
        assert_eq!(Period::Hour.start(LATE_MARCH), LATE_MARCH - 1_800);
        assert_eq!(Period::Day.next(Period::Day.start(LATE_MARCH)), APRIL);
        assert_eq!(Period::Month.next(Period::Month.start(LATE_MARCH)), APRIL);
    }

    #[test]
    fn an_hour_adds_to_its_day_and_month() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let counts = Counts {
            requests: 2,
            ..Counts::default()
        };
        let hour = Period::Hour.start(LATE_MARCH);

        store.add(7, hour, &counts).unwrap();
        store.add(7, hour, &counts).unwrap();
        store.add(7, APRIL, &counts).unwrap();

        let requests = |period| -> Vec<(i64, u64)> {
            let rows = store.rows(7, period, 0).unwrap();
            rows.into_iter()
                .map(|(start, row)| (start, row.requests))
                .collect()
        };
        assert_eq!(requests(Period::Hour), [(hour, 4), (APRIL, 2)]);
        let march = Period::Month.start(LATE_MARCH);
        assert_eq!(requests(Period::Month), [(march, 4), (APRIL, 2)]);
    }
}
