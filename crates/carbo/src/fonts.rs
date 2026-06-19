use std::{
  fmt,
  sync::{Mutex, MutexGuard},
};

use fontique::{
  Attributes, Collection, CollectionOptions, GenericFamily, QueryFamily,
  QueryFont, QueryStatus, SourceCache, SourceCacheOptions,
};
use tracing::instrument;

pub struct FontContext {
  // these both have internal sharing mechanisms but every method they have
  // takes a mutable reference, so we use a mutex to be able to use them
  // immutably.
  collection: Mutex<Collection>,
  cache:      Mutex<SourceCache>,
}

impl fmt::Debug for FontContext {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("FontContext")
      .field("collection", &"...")
      .field("source_cache", &"...")
      .finish()
  }
}

impl Clone for FontContext {
  fn clone(&self) -> Self {
    let (collection, cache) = self.get_lock();
    Self {
      collection: Mutex::new(collection.clone()),
      cache:      Mutex::new(cache.clone()),
    }
  }
}

impl FontContext {
  #[instrument]
  pub fn new() -> Self {
    let collection = Collection::new(CollectionOptions {
      shared:       true,
      system_fonts: false,
    });
    let source_cache = SourceCache::new(SourceCacheOptions { shared: true });

    Self {
      collection: Mutex::new(collection),
      cache:      Mutex::new(source_cache),
    }
  }

  #[instrument(skip_all)]
  fn get_lock(
    &self,
  ) -> (MutexGuard<'_, Collection>, MutexGuard<'_, SourceCache>) {
    (
      self
        .collection
        .lock()
        .expect("font collection mutex poisoned"),
      self.cache.lock().expect("font source cache mutex poisoned"),
    )
  }

  #[instrument(skip_all, fields(family))]
  pub fn resolve_face(
    &self,
    family: Option<&str>,
    attrs: Attributes,
  ) -> Option<QueryFont> {
    let (mut collection, mut cache) = self.get_lock();

    // build query
    let mut query = collection.query(&mut cache);
    let mut family_desc = vec![QueryFamily::Generic(GenericFamily::Monospace)];
    if let Some(family) = family {
      family_desc.insert(0, QueryFamily::Named(family));
    }
    query.set_families(family_desc);
    query.set_attributes(attrs);

    // search for families
    let mut found = None;
    query.matches_with(|qf| {
      found = Some(qf.clone());
      QueryStatus::Stop
    });

    found
  }

  #[instrument(skip_all)]
  pub fn load_system_fonts(&mut self) {
    let (mut collection, _cache) = self.get_lock();
    collection.load_system_fonts();
  }
}
