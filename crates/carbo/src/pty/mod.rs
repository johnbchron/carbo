mod state;

use std::{
  io::{self, Read, Write},
  num::{NonZeroU16, NonZeroUsize},
  sync::mpsc,
  time::{Duration, Instant},
};

use miette::{Context, IntoDiagnostic};
use portable_pty::ChildKiller;
use tracing::{field, info_span};

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
    slave_command.args(["-c", "yes", LIPSUM]);
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

    let (pty_command_tx, pty_command_rx) = mpsc::channel();

    let pty = Pty {
      pty_command_rx,
      writer,
      state: PtyState::new(rows, cols, scrollback),
      event_tx: event_tx.clone(),
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
    };

    Ok((handle, pty_state_view))
  }

  fn run(&mut self) -> miette::Result<()> {
    let mut pending_out = Vec::with_capacity(COALESCE_MAX_BYTES);
    loop {
      let entered = info_span!("await_pty_command").entered();
      let Ok(first) = self.pty_command_rx.recv() else {
        break;
      };
      drop(entered);

      let entered = info_span!("pty_command_dispatch").entered();
      // Coalesce a short burst of commands so a fast-spewing slave doesn't make
      // us re-parse and re-snapshot for every tiny chunk. We keep waiting (up
      // to a tiny window) for more, but flush early once the buffer is full.
      let deadline = Instant::now() + COALESCE_WINDOW;
      pending_out.clear();
      let mut cmd = Some(first);

      while let Some(command) = cmd {
        match command {
          PtyCommand::Input(b) => {
            self
              .writer
              .write_all(&b)
              .into_diagnostic()
              .context("failed to write to thread")?;
          }
          PtyCommand::Output(b) => {
            pending_out.extend_from_slice(&b);
          }
          PtyCommand::Resize { rows, cols } => {
            self.state.resize(rows, cols);
          }
        }

        if pending_out.len() >= COALESCE_MAX_BYTES {
          break;
        }
        // `recv_timeout` returns immediately while commands are queued, so this
        // only actually sleeps once we've drained the backlog.
        cmd = match deadline.checked_duration_since(Instant::now()) {
          Some(remaining) => self.pty_command_rx.recv_timeout(remaining).ok(),
          None => break,
        };
      }
      drop(entered);

      if !pending_out.is_empty() {
        self.state.process_input(&pending_out);

        let snapshot = self.state.snapshot();
        let _ = self.event_tx.try_event(Event::PtySnapshot(snapshot));
      }
    }

    Ok(())
  }
}

#[derive(Debug)]
pub struct PtyHandle {
  pty_command_tx: mpsc::Sender<PtyCommand>,
  child_killer:   Box<dyn ChildKiller + Send>,
}

impl PtyHandle {
  pub fn kill_child(&mut self) -> std::io::Result<()> {
    tracing::info!("killing pty child");
    self.child_killer.kill()
  }

  pub fn resize(&self, rows: NonZeroU16, cols: NonZeroU16) {
    let _ = self.pty_command_tx.send(PtyCommand::Resize { rows, cols });
  }
}

pub enum PtyCommand {
  /// Bytes to send to the PTY slave as input to the process.
  Input(Vec<u8>),
  /// Bytes sent from the PTY master as output from the process.
  Output(Vec<u8>),
  Resize {
    rows: NonZeroU16,
    cols: NonZeroU16,
  },
}

/// How long the pty state thread lets commands accumulate before flushing a
/// coalesced batch to the parser.
const COALESCE_WINDOW: Duration = Duration::from_micros(100);
/// Once a coalesced batch reaches this size we flush it immediately rather than
/// waiting for the timer; keeps a single parse/snapshot from growing unbounded.
const COALESCE_MAX_BYTES: usize = 256 * 1024;

fn run_reader(
  mut reader: Box<dyn Read + Send>,
  pty_command_tx: mpsc::Sender<PtyCommand>,
  event_tx: EventSender,
) {
  let mut buf = [0u8; 64 * 1024];
  loop {
    let read_span = info_span!("read_pty_master", len = field::Empty);
    let entered = read_span.enter();
    let result = reader.read(&mut buf);
    drop(entered);

    let _entered = info_span!("dispatch_pty_read").entered();
    match result {
      // an EOF means the child closed the slave side
      Ok(0) => {
        tracing::debug!("pty reader reached EOF");
        break;
      }
      Ok(n) => {
        read_span.record("len", n);
        let result = pty_command_tx.send(PtyCommand::Output(buf[..n].to_vec()));
        if result.is_err() {
          // the pty state thread has exited
          break;
        }
      }
      // a signal interrupted the read; try again
      Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
      // sometimes an IO error just means a normal exit
      Err(e) => {
        tracing::debug!(error = ?e, "pty reader received IO error; exiting");
        break;
      }
    }
  }

  let _ = event_tx.try_event(Event::PtyExited);
}
