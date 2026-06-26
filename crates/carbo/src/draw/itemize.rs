use smol_str::SmolStr;
use tracing::{info_span, instrument};
use vt100::Cell;

use super::FrameInput;

/// An estimation of how many cells will be covered by a single run.
const HEURISTIC_CELLS_PER_RUN: usize = 10;

#[derive(Default)]
pub struct ItemizerPersistentResources {
  runs: Vec<TextRun>,
}

#[derive(Debug)]
pub enum ClusterWidth {
  Single,
  Double,
}

#[derive(Debug)]
pub struct GraphemeCluster {
  contents: SmolStr,
  width:    ClusterWidth,
}

impl GraphemeCluster {
  fn from_cell(cell: &Cell) -> Self {
    GraphemeCluster {
      contents: SmolStr::new(cell.contents()),
      width:    match cell.is_wide() {
        true => ClusterWidth::Double,
        false => ClusterWidth::Single,
      },
    }
  }

  fn contents(&self) -> &str { &self.contents }
}

enum ItemizerStateMachine {
  /// Starting.
  Start,
  /// Text run has starting; eating normal characters.
  TextRunInProgress(TextRun),
  /// Eating empty cells.
  EatingEmpty,
}

#[derive(Debug)]
pub struct TextRun {
  /// The characters to render in this run.
  clusters:     Vec<GraphemeCluster>,
  /// The cell coords (x, y) where this run starts.
  start:        (u16, u16),
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
  /// Starts a new text run. The first cell must be a normal or wide cell, not a
  /// wide continuation or empty cell.
  fn start_run(first_cell: &Cell, first_cell_pos: (u16, u16)) -> Self {
    let mut clusters = Vec::with_capacity(HEURISTIC_CELLS_PER_RUN);

    debug_assert!(
      first_cell.has_contents(),
      "first cell in text run doesn't have contents"
    );
    debug_assert!(
      !first_cell.is_wide_continuation(),
      "first cell in text run is a wide continuation"
    );
    clusters.push(GraphemeCluster {
      contents: SmolStr::new(first_cell.contents()),
      width:    match first_cell.is_wide() {
        true => ClusterWidth::Double,
        false => ClusterWidth::Single,
      },
    });

    TextRun {
      clusters,
      start: first_cell_pos,
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

  /// Push a grapheme cluster.
  fn push_cluster(&mut self, cluster: GraphemeCluster) {
    self.clusters.push(cluster);
  }

  /// Returns true if all clusters are whitespace.
  fn is_visually_empty(&self) -> bool {
    self
      .clusters
      .iter()
      .any(|c| !c.contents().trim().is_empty())
  }
}

impl FrameInput {
  #[instrument(skip_all)]
  pub fn itemize_text_runs<'a>(
    &self,
    persist: &'a mut ItemizerPersistentResources,
  ) -> &'a [TextRun] {
    let screen = self.pty.screen();
    let (row_count, col_count) = screen.size();

    // clear the last runs & allocate if needed
    persist.runs.clear();
    let guessed_run_count =
      row_count as usize * (col_count as usize / HEURISTIC_CELLS_PER_RUN);
    info_span!("allocate_text_run_storage")
      .in_scope(|| persist.runs.reserve(guessed_run_count));

    // iterate through all rows.
    for row_idx in 0..row_count {
      let _row_span = info_span!("itemize_row").entered();
      // the state machine runs per row, since rows always break a run
      let mut state = ItemizerStateMachine::Start;

      // iterate through all cells in the grid
      for x_cursor in 0..col_count {
        // get the current cell
        let cell = screen
          .cell(row_idx, x_cursor)
          .expect("could not get grid cell during run itemizing");

        // transition the state machine with the current cell
        state = match state {
          ItemizerStateMachine::Start => {
            debug_assert!(
              !cell.is_wide_continuation(),
              "row started with wide continuation"
            );
            match cell {
              // begin the run
              c if c.has_contents() => {
                let run = TextRun::start_run(cell, (x_cursor, row_idx));
                ItemizerStateMachine::TextRunInProgress(run)
              }
              // eat the empty cell
              _ => ItemizerStateMachine::EatingEmpty,
            }
          }
          ItemizerStateMachine::TextRunInProgress(mut run) => {
            match cell {
              // the cell is empty, so emit the run
              c if !c.has_contents() => {
                persist.runs.push(run);
                ItemizerStateMachine::EatingEmpty
              }
              // skip double-wide continuation cells
              c if c.is_wide_continuation() => {
                ItemizerStateMachine::TextRunInProgress(run)
              }
              // the style matches, so push cluster to the run and continue
              c if run.should_coalesce(c) => {
                run.push_cluster(GraphemeCluster::from_cell(c));
                ItemizerStateMachine::TextRunInProgress(run)
              }
              // the style doesn't match, so emit the run and start a new one
              _ => {
                persist.runs.push(run);
                let new_run = TextRun::start_run(cell, (x_cursor, row_idx));
                ItemizerStateMachine::TextRunInProgress(new_run)
              }
            }
          }
          ItemizerStateMachine::EatingEmpty => {
            match cell {
              // the cell is empty, so continue
              c if !c.has_contents() => ItemizerStateMachine::EatingEmpty,
              // the cell is not empty, so start a new run
              _ => {
                let new_run = TextRun::start_run(cell, (x_cursor, row_idx));
                ItemizerStateMachine::TextRunInProgress(new_run)
              }
            }
          }
        };
      }

      // end the state machine
      match state {
        ItemizerStateMachine::Start => {
          debug_assert!(false, "the itemizer state machine never transitioned");
        }
        ItemizerStateMachine::TextRunInProgress(text_run) => {
          // push the last run if it's not all whitespace
          if !text_run.is_visually_empty() {
            persist.runs.push(text_run);
          }
        }
        // last cell was empty, no need to do anything
        ItemizerStateMachine::EatingEmpty => {}
      };
    }

    &persist.runs
  }
}
