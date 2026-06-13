use vt100::Cell;

use super::FrameInput;

/// An estimation of how many cells will be covered by a single run.
const HEURISTIC_CELLS_PER_RUN: usize = 10;

pub struct ItemizerPersistentResources {
  runs: Vec<TextRun>,
}

pub struct TextRun {
  /// The characters to render in this run.
  chars:        String,
  /// The cell coords (x, y) where this run starts.
  start:        (u16, u16),
  /// The number of cells this run covers.
  cell_width:   u16,
  /// The foreground color to draw this run with.
  effective_fg: vt100::Color,
  /// The boldness to draw this run with.
  bold:         bool,
  /// The dimness to draw this run with.
  dim:          bool,
  /// The italicness to draw this run with.
  italic:       bool,
}

impl TextRun {
  fn start_run(first_cell: &Cell, first_cell_pos: (u16, u16)) -> Self {
    let mut chars = String::with_capacity(HEURISTIC_CELLS_PER_RUN);
    chars.push_str(first_cell.contents());

    TextRun {
      chars,
      start: first_cell_pos,
      cell_width: 1,
      effective_fg: if first_cell.inverse() {
        first_cell.bgcolor()
      } else {
        first_cell.fgcolor()
      },
      bold: first_cell.bold(),
      dim: first_cell.dim(),
      italic: first_cell.italic(),
    }
  }

  fn should_coalesce(&self, cell: &Cell) -> bool {
    let cell_effective_fg = if cell.inverse() {
      cell.bgcolor()
    } else {
      cell.fgcolor()
    };

    cell_effective_fg == self.effective_fg
      && cell.bold() == self.bold
      && cell.dim() == self.dim
      && cell.italic() == self.italic
  }

  fn push_to_run(&mut self, cell: &Cell) {
    self.chars.push_str(cell.contents());
    self.cell_width += 1;
  }

  fn is_visually_empty(&self) -> bool { self.chars.trim().is_empty() }
}

impl FrameInput {
  pub fn itemize_text_runs<'a>(
    &self,
    persist: &'a mut ItemizerPersistentResources,
  ) -> &'a [TextRun] {
    let screen = self.pty.screen();
    let (row_count, col_count) = screen.size();

    // clear the last runs & allocate if needed
    persist.runs.clear();
    let guessed_run_count =
      (row_count as usize * col_count as usize) / HEURISTIC_CELLS_PER_RUN;
    persist.runs.reserve(guessed_run_count);

    for row_idx in 0..row_count {
      let first_cell = screen
        .cell(row_idx, 0)
        .expect("could not get first grid cell in row during run itemizing");

      // start the run with the first cell in the row
      let mut current_run = TextRun::start_run(first_cell, (0, row_idx));

      for x_cursor in 1..col_count {
        // get the current cell
        let cell = screen
          .cell(row_idx, x_cursor)
          .expect("could not get grid cell during run itemizing");

        // if the previous was a double-wide, just increment width & move on
        if cell.is_wide_continuation() {
          current_run.cell_width += 1;
          continue;
        }

        // add the character if it's the same style, otherwise start a new run
        if current_run.should_coalesce(cell) {
          current_run.push_to_run(cell);
        } else {
          let new_run = TextRun::start_run(cell, (x_cursor, row_idx));
          let completed_run = std::mem::replace(&mut current_run, new_run);

          if !completed_run.is_visually_empty() {
            persist.runs.push(completed_run);
          }
        }
      }

      if !current_run.is_visually_empty() {
        persist.runs.push(current_run);
      }
    }

    &persist.runs
  }
}
