use std::{sync::mpsc, thread::JoinHandle};

use miette::{Context, IntoDiagnostic};

pub struct Pty {
  parser:         vte::Parser,
  pty_command_rx: mpsc::Receiver<PtyCommand>,
}

impl Pty {
  pub fn launch() -> miette::Result<PtyHandle> {
    let (pty_command_tx, pty_command_rx) = mpsc::channel();

    let parser = vte::Parser::new();

    let pty = Pty {
      parser,
      pty_command_rx,
    };

    let join_handle = std::thread::Builder::new()
      .name("pty".into())
      .spawn(move || {
        let mut pty = pty;
        pty.run().unwrap();
      })
      .into_diagnostic()
      .context("failed to launch pty thread")?;

    let handle = PtyHandle {
      _join_handle: join_handle,
      pty_command_tx,
    };

    Ok(handle)
  }

  fn run(&mut self) -> miette::Result<()> {
    while let Ok(command) = self.pty_command_rx.recv() {
      match command {
        PtyCommand::Input(items) => todo!(),
        PtyCommand::Output(items) => todo!(),
      }
    }

    Ok(())
  }
}

struct PtyState {}

pub struct PtyHandle {
  _join_handle:   JoinHandle<()>,
  pty_command_tx: mpsc::Sender<PtyCommand>,
}

pub enum PtyCommand {
  /// Bytes to send to the PTY slave as input to the process.
  Input(Vec<u8>),
  /// Bytes sent from the PTY master as output from the process.
  Output(Vec<u8>),
}
