pub struct AppConfig {
  pub font_size: f32,
}

impl Default for AppConfig {
  fn default() -> Self { Self { font_size: 16.0 } }
}
