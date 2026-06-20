use std::{
  fmt,
  sync::{Arc, Mutex, MutexGuard},
};

use fontique::{
  Attributes, Collection, CollectionOptions, FontStyle, FontWeight, FontWidth,
  GenericFamily, QueryFamily, QueryFont, QueryStatus, SourceCache,
  SourceCacheOptions,
};
use skrifa::{FontRef, MetadataProvider};
use tracing::instrument;
use vello::peniko::linebender_resource_handle::FontData;

use crate::app::state::config::FontConfig;

#[derive(Clone)]
pub struct FontContext {
  collection: Arc<Mutex<Collection>>,
  cache:      Arc<Mutex<SourceCache>>,
}

#[derive(Debug)]
pub struct TerminalFonts {
  pub family_name:  String,
  pub regular:      FontData,
  pub cell_metrics: CellMetrics,
}

/// All lengths in logical px.
#[derive(Clone, Copy, Debug)]
pub struct CellMetrics {
  advance:  f32,
  ascent:   f32,
  descent:  f32,
  line_gap: f32,
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
  #[instrument("new_font_context")]
  pub fn new() -> Self {
    let collection = Collection::new(CollectionOptions {
      shared:       true,
      system_fonts: false,
    });
    let source_cache = SourceCache::new(SourceCacheOptions { shared: true });

    Self {
      collection: Arc::new(Mutex::new(collection)),
      cache:      Arc::new(Mutex::new(source_cache)),
    }
  }

  #[instrument("font_context_get_lock", skip_all)]
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
  pub fn resolve_terminal_fonts(
    &self,
    font_config: &FontConfig,
  ) -> TerminalFonts {
    let regular_attrs =
      Attributes::new(FontWidth::NORMAL, FontStyle::Normal, FontWeight::NORMAL);
    let regular = self
      .resolve_face(font_config.font_family.as_deref(), regular_attrs)
      .expect("failed to resolve a regular font");

    let family_name = self
      .get_lock()
      .0
      .family_name(regular.family.0)
      .expect("could not find family name")
      .to_owned();

    let font_data = FontData {
      data:  regular.blob.clone(),
      index: regular.index,
    };

    let font_ref =
      FontRef::from_index(font_data.data.as_ref(), font_data.index)
        .expect("failed to read font data");

    // use default axis location
    let axis_collection = font_ref.axes();
    let location = axis_collection.location::<&[(&str, f32)]>(&[]);

    // a point is 1/72 of an inch, and standard DPI is 96 PPI.
    let size =
      skrifa::instance::Size::new(font_config.font_size_pt / 72.0 * 96.0);

    let metrics = font_ref.metrics(size, &location);
    let glyph_metrics = font_ref.glyph_metrics(size, &location);
    let rep_glyph_id = font_ref
      .charmap()
      .map('M')
      .expect("font does not contain M glyph");
    let advance = glyph_metrics.advance_width(rep_glyph_id).unwrap();

    let cell_metrics = CellMetrics {
      advance,
      ascent: metrics.ascent,
      descent: metrics.descent,
      line_gap: metrics.leading,
    };

    TerminalFonts {
      family_name,
      regular: font_data,
      cell_metrics,
    }
  }

  #[instrument(skip_all)]
  pub fn load_system_fonts(&mut self) {
    let (mut collection, _cache) = self.get_lock();
    collection.load_system_fonts();
  }
}
