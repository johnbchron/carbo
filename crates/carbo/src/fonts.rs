use std::sync::{Arc, Mutex};

use fontique::{Collection, CollectionOptions, SourceCache};

pub struct FontManager {
  inner: Arc<Mutex<Inner>>,
}

struct Inner {
  collection:   Collection,
  source_cache: SourceCache,
}

impl FontManager {
  pub fn new() -> Self {
    let collection = Collection::new(CollectionOptions {
      shared:       false,
      system_fonts: true,
    });
    let source_cache = SourceCache::default();

    let inner = Inner {
      collection,
      source_cache,
    };

    Self {
      inner: Arc::new(Mutex::new(inner)),
    }
  }
}
