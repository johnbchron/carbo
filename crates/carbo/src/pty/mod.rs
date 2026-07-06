mod state;

use std::{
  io::{self, Read, Write},
  num::{NonZeroU16, NonZeroUsize},
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
  },
  time::{Duration, Instant},
};

use miette::{Context, IntoDiagnostic};
use portable_pty::ChildKiller;
use sharded_slab::{Clear, Pool, pool::OwnedRef};
use tracing::{field::Empty, info_span};

pub use self::state::{PtyState, PtyStateView};
use crate::{event::Event, event_sender::EventSender};

const LIPSUM: &str =
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Donec sit amet \
   iaculis turpis, vel convallis ante. Donec commodo suscipit purus, sit amet \
   tincidunt dui pretium hendrerit. Curabitur elementum mauris eu elementum \
   malesuada. Maecenas dignissim erat libero, sed ultrices ipsum euismod quis.";

pub struct Pty {
  pty_command_rx: mpsc::Receiver<PtyCommand>,
  writer:         Box<dyn Write + Send>,
  state:          PtyState,
  event_tx:       EventSender,
  snapshot_req:   Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct PtySpawnArguments {
  pub rows:       NonZeroU16,
  pub cols:       NonZeroU16,
  pub scrollback: NonZeroUsize,
}

impl Pty {
  pub fn launch(
    args: PtySpawnArguments,
    event_tx: EventSender,
  ) -> miette::Result<(PtyHandle, PtyStateView)> {
    let PtySpawnArguments {
      rows,
      cols,
      scrollback,
    } = args;

    let entered = info_span!("pty_open").entered();
    let pty_system = portable_pty::native_pty_system();
    let portable_pty::PtyPair { slave, master } = pty_system
      .openpty(portable_pty::PtySize {
        rows:         rows.get(),
        cols:         cols.get(),
        pixel_width:  0,
        pixel_height: 0,
      })
      .map_err(|e| miette::miette!(e))
      .context("failed to open pty")?;
    let mut slave_command = portable_pty::CommandBuilder::new("bash");
    slave_command.args(["-c", "yes"]);
    let child = slave
      .spawn_command(slave_command)
      .map_err(|e| miette::miette!(e))
      .context("failed to spawn command in PTY slave")?;
    let child_killer = child.clone_killer();
    let reader = master
      .try_clone_reader()
      .map_err(|e| miette::miette!(e))
      .context("failed to get PTY reader from master side")?;
    let writer = master
      .take_writer()
      .map_err(|e| miette::miette!(e))
      .context("failed to take PTY writer from master side")?;
    drop(entered);

    let (pty_command_tx, pty_command_rx) = mpsc::sync_channel(32);
    let snapshot_req = Arc::new(AtomicBool::new(true));

    let pty = Pty {
      pty_command_rx,
      writer,
      state: PtyState::new(rows, cols, scrollback),
      event_tx: event_tx.clone(),
      snapshot_req: snapshot_req.clone(),
    };

    let pty_state_view = pty.state.snapshot();

    std::thread::Builder::new()
      .name("pty_state".into())
      .spawn({
        let event_tx = event_tx.clone();
        move || {
          let mut pty = pty;
          if let Err(e) = pty.run() {
            event_tx.event(Event::CriticalFailure {
              message: "the pty state thread failed".into(),
              error:   e,
            });
          }
        }
      })
      .into_diagnostic()
      .context("failed to launch pty state thread")?;

    std::thread::Builder::new()
      .name("pty_reader".into())
      .spawn({
        let pty_command_tx = pty_command_tx.clone();
        move || run_reader(reader, pty_command_tx, event_tx)
      })
      .into_diagnostic()
      .context("failed to launch pty reader thread")?;

    let handle = PtyHandle {
      pty_command_tx,
      child_killer,
      snapshot_req,
    };

    Ok((handle, pty_state_view))
  }

  /// Just a simple command dispatcher. In a loop, it accepts commands and runs
  /// them. Exits when all pty command senders are dropped, or the app is
  /// dropped.
  fn run(&mut self) -> miette::Result<()> {
    loop {
      let entered = info_span!("await_pty_command").entered();
      let Ok(command) = self.pty_command_rx.recv() else {
        break;
      };
      drop(entered);

      let _entered = info_span!("pty_command_dispatch").entered();
      match command {
        PtyCommand::Input(b) => {
          self
            .writer
            .write_all(&b)
            .into_diagnostic()
            .context("failed to write to thread")?;
        }
        PtyCommand::Output(b) => {
          self.state.process_input(&b.0);
        }
        PtyCommand::Resize { rows, cols } => {
          self.state.resize(rows, cols);
        }
      }

      // send a snapshot if requested
      if self.snapshot_req.load(Ordering::Relaxed) {
        self.snapshot_req.store(false, Ordering::Relaxed);
        let res = self
          .event_tx
          .try_event(Event::PtySnapshot(self.state.snapshot()));
        if res.is_err() {
          break;
        }
      }
    }

    Ok(())
  }
}

#[derive(Debug)]
pub struct PtyHandle {
  pty_command_tx: mpsc::SyncSender<PtyCommand>,
  child_killer:   Box<dyn ChildKiller + Send>,
  snapshot_req:   Arc<AtomicBool>,
}

impl PtyHandle {
  pub fn kill_child(&mut self) -> std::io::Result<()> {
    tracing::info!("killing pty child");
    self.child_killer.kill()
  }

  pub fn resize(&self, rows: NonZeroU16, cols: NonZeroU16) {
    let _ = self.pty_command_tx.send(PtyCommand::Resize { rows, cols });
  }

  pub fn request_snapshot(&self) {
    self.snapshot_req.store(true, Ordering::Relaxed);
  }
}

enum PtyCommand {
  /// Bytes to send to the PTY slave as input to the process.
  Input(Vec<u8>),
  /// Bytes sent from the PTY master as output from the process.
  Output(OwnedRef<OutputChunk>),
  Resize {
    rows: NonZeroU16,
    cols: NonZeroU16,
  },
}

/// How long the pty state thread lets commands accumulate before flushing a
/// coalesced batch to the parser.
const COALESCE_WINDOW: Duration = Duration::from_micros(500);
/// Once a coalesced batch reaches this size we flush it immediately rather than
/// waiting for the timer; keeps a single parse/snapshot from growing unbounded.
const COALESCE_MAX_BYTES: usize = 256 * 1024;

/// Reused allocation for PTY output.
struct OutputChunk(Vec<u8>);

impl Default for OutputChunk {
  fn default() -> Self { Self(Vec::with_capacity(COALESCE_MAX_BYTES)) }
}

impl Clear for OutputChunk {
  fn clear(&mut self) { self.0.clear() }
}

/// A reader thread that coalesces reads from the PTY output.
fn run_reader(
  mut reader: Box<dyn Read + Send>,
  pty_command_tx: mpsc::SyncSender<PtyCommand>,
  event_tx: EventSender,
) {
  // make a pool for reusing the output allocations
  let pool: Arc<Pool<OutputChunk>> = Arc::new(Pool::new());

  'outer: loop {
    // get a new output allocation
    let mut buf = pool.clone().create_owned().unwrap();
    // set the deadline. only respected if we're behind on sending.
    let deadline = Instant::now() + COALESCE_WINDOW;

    // read in a loop until we exceed the deadline or data limit
    let entered = info_span!("coalesce_loop", chunk_len = Empty).entered();
    'coalesce: loop {
      // mark where the data ends and add more space to the chunk
      let start = buf.0.len();
      buf.0.resize((start + 64 * 1024).min(COALESCE_MAX_BYTES), 0);

      // read into the new empty space
      let result = info_span!("read_pty_master")
        .in_scope(|| reader.read(&mut buf.0[start..]));

      // bail if the read went wrong
      let n = match result {
        Ok(0) => {
          tracing::debug!("pty reader reached EOF");
          break 'outer;
        }
        Ok(n) => n,
        // a signal interrupted the read; try again
        Err(e) if e.kind() == io::ErrorKind::Interrupted => 0,
        Err(e) => {
          tracing::error!(error = ?e, "pty reader received IO error; exiting");
          break 'outer;
        }
      };

      // trim down to the data region
      buf.0.truncate(start + n);

      // quit if needed
      let over_size = buf.0.len() >= COALESCE_MAX_BYTES;
      let over_time = deadline.checked_duration_since(Instant::now()).is_none();
      if over_size || over_time {
        break 'coalesce;
      }
    }
    entered.record("chunk_len", buf.0.len());
    drop(entered);

    // send the chunk off
    let chunk_buf =
      std::mem::replace(&mut buf, pool.clone().create_owned().unwrap());
    let res = info_span!("send_pty_output_chunk").in_scope(|| {
      pty_command_tx.send(PtyCommand::Output(chunk_buf.downgrade()))
    });
    if res.is_err() {
      break;
    };
  }

  let _ = event_tx.try_event(Event::PtyExited);
}
