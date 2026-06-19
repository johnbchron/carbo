use std::{
  fmt,
  sync::{Mutex, MutexGuard},
};

use fontique::{
  Attributes, Collection, CollectionOptions, GenericFamily, QueryFamily,
  QueryFont, QueryStatus, SourceCache, SourceCacheOptions,
};

pub struct FontContext {
  // these both have internal sharing mechanisms but every method they have
  // takes a mutable reference, so we use a mutex to be able to use them
  // immutably.
  collection:   Mutex<Collection>,
  source_cache: Mutex<SourceCache>,
}

impl fmt::Debug for FontContext {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("FontContext")
      .field("collection", &"...")
      .field("source_cache", &"...")
      .finish()
  }
}

impl FontContext {
  pub fn new() -> Self {
    let collection = Collection::new(CollectionOptions {
      shared:       true,
      system_fonts: true,
    });
    let source_cache = SourceCache::new(SourceCacheOptions { shared: true });

    Self {
      collection:   Mutex::new(collection),
      source_cache: Mutex::new(source_cache),
    }
  }

  fn get_mut(
    &self,
  ) -> (MutexGuard<'_, Collection>, MutexGuard<'_, SourceCache>) {
    (
      self
        .collection
        .lock()
        .expect("font collection mutex poisoned"),
      self
        .source_cache
        .lock()
        .expect("font source cache mutex poisoned"),
    )
  }

  pub fn resolve_face(
    &self,
    family: Option<&str>,
    attrs: Attributes,
  ) -> Option<QueryFont> {
    let (mut collection, mut cache) = self.get_mut();

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
}
