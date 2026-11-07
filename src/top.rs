use rustc_hash::FxHashMap;

use serde::{Deserialize, Serialize};

const CAPACITY: usize = 100;
const MAX_VALUE_LENGTH: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Top(FxHashMap<String, Count>);

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Count {
    pub count: u64,
    pub error: u64,
}

impl Top {
    pub fn add(&mut self, value: &str) {
        let value = &value[..value.floor_char_boundary(MAX_VALUE_LENGTH)];
        if let Some(count) = self.0.get_mut(value) {
            count.count += 1;
            return;
        }
        let inherited = if self.0.len() < CAPACITY {
            0
        } else {
            self.remove_smallest()
        };
        let count = Count {
            count: inherited + 1,
            error: inherited,
        };
        self.0.insert(value.to_owned(), count);
    }

    pub fn merge(&mut self, other: &Top) {
        let (floor, other_floor) = (self.floor(), other.floor());
        for (value, count) in &mut self.0 {
            if !other.0.contains_key(value) {
                count.count += other_floor;
                count.error += other_floor;
            }
        }
        for (value, theirs) in &other.0 {
            match self.0.get_mut(value) {
                Some(count) => {
                    count.count += theirs.count;
                    count.error += theirs.error;
                }
                None => {
                    let count = Count {
                        count: floor + theirs.count,
                        error: floor + theirs.error,
                    };
                    self.0.insert(value.clone(), count);
                }
            }
        }
        if self.0.len() > CAPACITY {
            let mut ranked: Vec<_> = self.0.drain().collect();
            ranked.sort_by_key(|(_, count)| std::cmp::Reverse(count.count));
            ranked.truncate(CAPACITY);
            self.0 = ranked.into_iter().collect();
        }
    }

    pub fn ranked(&self) -> Vec<(&str, Count)> {
        let mut ranked: Vec<_> = self.0.iter().map(|(v, c)| (v.as_str(), *c)).collect();
        ranked.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
        ranked
    }

    fn floor(&self) -> u64 {
        if self.0.len() < CAPACITY {
            return 0;
        }
        self.0.values().map(|count| count.count).min().unwrap_or(0)
    }

    fn remove_smallest(&mut self) -> u64 {
        let (value, count) = self
            .0
            .iter()
            .min_by_key(|(_, count)| count.count)
            .map(|(value, count)| (value.clone(), count.count))
            .expect("only called when every counter is in use");
        self.0.remove(&value);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(top: &Top) -> Vec<(&str, u64)> {
        top.ranked()
            .into_iter()
            .map(|(v, c)| (v, c.count))
            .collect()
    }

    #[test]
    fn counts_exactly_while_counters_are_free() {
        let mut top = Top::default();
        for value in ["a", "b", "a"] {
            top.add(value);
        }

        assert_eq!(counts(&top), [("a", 2), ("b", 1)]);
    }

    #[test]
    fn a_frequent_value_survives_a_long_tail() {
        let mut top = Top::default();
        for i in 0..10_000 {
            top.add(&format!("noise{i}"));
            if i % 10 == 0 {
                top.add("frequent");
            }
        }

        let (value, count) = top.ranked()[0];
        assert_eq!(value, "frequent");
        assert!(count.count - count.error <= 1_000 && 1_000 <= count.count);
    }

    #[test]
    fn merging_adds_counts() {
        let (mut first, mut second) = (Top::default(), Top::default());
        first.add("a");
        second.add("a");
        second.add("b");

        first.merge(&second);

        assert_eq!(counts(&first), [("a", 2), ("b", 1)]);
    }

    #[test]
    fn cuts_long_values() {
        let mut top = Top::default();

        top.add(&"a".repeat(1_000));

        assert_eq!(top.ranked()[0].0.len(), MAX_VALUE_LENGTH);
    }
}
