use std::{fmt, sync::Mutex};

use fontique::{
  Collection, CollectionOptions, SourceCache, SourceCacheOptions,
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
}
