mod state;

use std::{
  io::{self, Read, Write},
  num::{NonZeroU16, NonZeroUsize},
  sync::mpsc,
};

use miette::{Context, IntoDiagnostic};
use portable_pty::ChildKiller;
use tracing::info_span;

pub use self::state::{PtyState, PtyStateView};
use crate::{event::Event, event_sender::EventSender};

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
    let slave_command = portable_pty::CommandBuilder::new("bash");
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

    let pty_state_view = pty.state.view();

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
    while let Ok(first) = self.pty_command_rx.recv() {
      let entered = info_span!("command_dispatch").entered();
      let mut pending_out = Vec::new();
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
        }
        cmd = self.pty_command_rx.try_recv().ok();
      }
      drop(entered);

      if !pending_out.is_empty() {
        let _entered = info_span!("parser_advance");
        self.state.process(&pending_out);
        let _ = self
          .event_tx
          .try_event(Event::PtySnapshot(self.state.view()));
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
}

pub enum PtyCommand {
  /// Bytes to send to the PTY slave as input to the process.
  Input(Vec<u8>),
  /// Bytes sent from the PTY master as output from the process.
  Output(Vec<u8>),
}

fn run_reader(
  mut reader: Box<dyn Read + Send>,
  pty_command_tx: mpsc::Sender<PtyCommand>,
  event_tx: EventSender,
) {
  let mut buf = [0u8; 64 * 1024];
  loop {
    let result =
      tracing::info_span!("read_master").in_scope(|| reader.read(&mut buf));
    let _entered = tracing::info_span!("dispatch_read").entered();
    match result {
      // an EOF means the child closed the slave side
      Ok(0) => {
        tracing::debug!("pty reader reached EOF");
        break;
      }
      Ok(n) => {
        // the pty state thread has exited
        let result = pty_command_tx.send(PtyCommand::Output(buf[..n].to_vec()));
        if result.is_err() {
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
