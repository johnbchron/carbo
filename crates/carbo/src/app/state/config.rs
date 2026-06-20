#[derive(Clone, Debug)]
pub struct AppConfig {
  pub font_config: FontConfig,
}

#[derive(Clone, Debug)]
pub struct FontConfig {
  pub font_size_pt: f32,
  pub font_family:  Option<String>,
}

impl Default for AppConfig {
  fn default() -> Self {
    Self {
      font_config: FontConfig {
        font_size_pt: 16.0,
        font_family:  None,
      },
    }
  }
}
