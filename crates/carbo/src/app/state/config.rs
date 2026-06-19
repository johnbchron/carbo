pub struct AppConfig {
  pub font_size:   f32,
  pub font_family: Option<String>,
}

impl Default for AppConfig {
  fn default() -> Self {
    Self {
      font_size:   16.0,
      font_family: None,
    }
  }
}
