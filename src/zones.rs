use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use serde::{Deserialize, Serialize};

use crate::store::Store;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Zone {
    #[serde(default)]
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub params: Vec<String>,
    pub token: String,
}

impl Zone {
    pub fn accepts(&self, token: Option<&str>) -> bool {
        token == Some(self.token.as_str())
    }

    pub fn counts(&self, path: &str) -> bool {
        self.paths.is_empty() || self.paths.iter().any(|counted| counted == path)
    }
}

pub enum SaveError {
    Invalid(String),
    Failed(anyhow::Error),
}

pub struct Zones {
    store: Arc<Store>,
    saved: RwLock<HashMap<u64, Zone>>,
}

impl Zones {
    pub fn load(store: Arc<Store>) -> anyhow::Result<Zones> {
        let saved = store.zones_saved()?;
        let saved = saved.into_iter().map(|zone| (zone.id, zone)).collect();
        Ok(Zones {
            store,
            saved: RwLock::new(saved),
        })
    }

    pub fn with<T>(&self, id: u64, f: impl FnOnce(Option<&Zone>) -> T) -> T {
        f(self.read().get(&id))
    }

    pub fn find(&self, name: &str) -> Option<Zone> {
        self.read().values().find(|zone| zone.name == name).cloned()
    }

    pub fn all(&self) -> Vec<Zone> {
        let mut zones: Vec<_> = self.read().values().cloned().collect();
        zones.sort_by(|a, b| a.name.cmp(&b.name));
        zones
    }

    pub fn add(&self, zone: Zone) -> Result<(), SaveError> {
        if self.with(zone.id, |existing| existing.is_some()) {
            return Err(SaveError::Invalid(format!("zone {} exists", zone.id)));
        }
        self.save(zone)
    }

    pub fn save(&self, zone: Zone) -> Result<(), SaveError> {
        let name = &zone.name;
        let allowed = |c: char| c.is_ascii_alphanumeric() || "._-".contains(c);
        if name.is_empty() || !name.chars().all(allowed) {
            return Err(SaveError::Invalid(format!("invalid name {name}")));
        }
        if zone.token.is_empty() {
            return Err(SaveError::Invalid("a token is required".to_owned()));
        }
        let mut saved = self
            .saved
            .write()
            .expect("no thread panics holding the lock");
        if saved
            .values()
            .any(|other| other.id != zone.id && other.name == *name)
        {
            return Err(SaveError::Invalid(format!("{name} is taken")));
        }
        self.store.save_zone(&zone).map_err(SaveError::Failed)?;
        saved.insert(zone.id, zone);
        Ok(())
    }

    fn read(&self) -> RwLockReadGuard<'_, HashMap<u64, Zone>> {
        self.saved
            .read()
            .expect("no thread panics holding the lock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_zones_survive_a_restart_and_names_stay_unique() {
        let dir = tempfile::tempdir().unwrap();
        let zones = Zones::load(Arc::new(Store::open(dir.path()).unwrap())).unwrap();
        let typeahead = Zone {
            id: 7,
            name: "typeahead".to_owned(),
            token: "secret".to_owned(),
            ..Zone::default()
        };

        assert!(zones.add(typeahead.clone()).is_ok());
        assert!(zones.add(typeahead.clone()).is_err());
        let same_name = Zone {
            id: 8,
            ..typeahead.clone()
        };
        assert!(zones.add(same_name).is_err());
        let no_token = Zone {
            id: 9,
            name: "other".to_owned(),
            ..Zone::default()
        };
        assert!(zones.add(no_token).is_err());
        drop(zones);

        let zones = Zones::load(Arc::new(Store::open(dir.path()).unwrap())).unwrap();
        assert_eq!(zones.all(), [typeahead]);
    }
}
