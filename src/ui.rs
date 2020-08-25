// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod commands;

use crate::contracts::{Bundle, DecodeError};

use ethers::types::{Transaction as EthTransaction, H256};

use rustyline::error::ReadlineError;
use rustyline::Editor;

pub use self::commands::{CommandKind, GetType, PoolType};

use std::fmt;
use std::thread::{self, JoinHandle};

use structopt::StructOpt;

use tokio::runtime::Handle;
use tokio::sync::mpsc::{self, Receiver, Sender};

#[derive(Debug, Clone)]
pub struct Events(Sender<Event>);

impl Events {
    pub async fn reply<E>(&mut self, cmd: &Command, kind: E)
    where
        E: Into<EventKind>,
    {
        let evt = Event {
            reply_to: Some(cmd.id),
            kind: kind.into(),
        };

        self.0.send(evt).await.expect("unable to send event");
    }

    pub async fn oob<E>(&mut self, kind: E)
    where
        E: Into<EventKind>,
    {
        let evt = Event {
            reply_to: None,
            kind: kind.into(),
        };

        self.0.send(evt).await.expect("unable to send event");
    }

    pub async fn bad_block(&mut self, block_hash: H256, error: crate::Error) {
        self.oob(EventKind::BadBlock(block_hash, error)).await;
    }

    pub async fn bad_bundle(&mut self, tx: EthTransaction) {
        self.oob(EventKind::BadBundle(tx)).await;
    }

    pub async fn good_bundle(&mut self, tx: EthTransaction) {
        self.oob(EventKind::GoodBundle(tx)).await;
    }

    pub async fn decode_error(&mut self, tx: EthTransaction, e: DecodeError) {
        self.oob(EventKind::DecodeError(tx, e)).await;
    }

    pub async fn pending_tx(&mut self, tx: H256) {
        self.oob(EventKind::PendingTransaction(tx)).await;
    }

    pub async fn new_block(&mut self, tx: H256) {
        self.oob(EventKind::NewBlock(tx)).await;
    }

    pub async fn get<S, V>(&mut self, cmd: &Command, name: S, value: V)
    where
        S: Into<String>,
        V: fmt::Display,
    {
        self.reply(cmd, EventKind::Get(name.into(), value.to_string()))
            .await;
    }
}

#[derive(Debug)]
pub struct Event {
    reply_to: Option<u8>,
    kind: EventKind,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[")?;

        match self.reply_to {
            Some(x) => write!(f, "{:02X}", x)?,
            None => write!(f, "--")?,
        }

        write!(f, "] {}", self.kind)
    }
}

#[derive(Debug)]
pub enum EventKind {
    Info(String),
    NewBlock(H256),
    BadBlock(H256, crate::Error),
    BadBundle(EthTransaction),
    GoodBundle(EthTransaction),
    DecodeError(EthTransaction, DecodeError),
    Broadcast(Bundle),
    PendingTransaction(H256),
    CommandError(crate::Error),
    PoolDrop(usize),
    PoolAdd(usize),
    Get(String, String),
}

impl From<&str> for EventKind {
    fn from(s: &str) -> Self {
        EventKind::Info(s.to_owned())
    }
}

impl From<String> for EventKind {
    fn from(s: String) -> Self {
        EventKind::Info(s)
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EventKind::Info(s) => write!(f, "{}", s),
            EventKind::BadBlock(bkhash, e) => {
                write!(f, "Failed to process block {}: {}", bkhash, e)
            }
            EventKind::BadBundle(tx) => write!(
                f,
                "Invalid transaction mined in {} (block #{})",
                tx.hash,
                tx.block_number.unwrap()
            ),
            EventKind::GoodBundle(tx) => write!(
                f,
                "Bundle mined in {} (block #{})",
                tx.hash,
                tx.block_number.unwrap()
            ),
            EventKind::DecodeError(tx, e) => {
                write!(f, "Unable to decode bundle for {}", tx.hash,)?;

                if let Some(bk) = tx.block_number {
                    write!(f, " (block #{})", bk)?;
                }

                write!(f, ": {}", e)
            }
            EventKind::Broadcast(bundle) => write!(
                f,
                concat!(
                    "Broadcasting bundle paying up to {} wei for gas with ",
                    "{} deposit(s), ",
                    "{} transfer(s), and ",
                    "{} withdrawal(s)"
                ),
                bundle.minimum_gas_price().unwrap_or_default(),
                bundle.claim.deposits.len(),
                bundle.transfers.len(),
                bundle.withdrawals.len(),
            ),
            EventKind::NewBlock(bk) => write!(f, "New Block: {}", bk,),
            EventKind::PendingTransaction(tx) => {
                write!(f, "New Pending Tx: {}", tx,)
            }
            EventKind::CommandError(e) => write!(f, "Command error: {}", e),
            EventKind::PoolDrop(c) => {
                write!(f, "Dropped {} transaction(s) from pool", c)
            }
            EventKind::PoolAdd(c) => {
                write!(f, "Added {} transaction(s) to pool", c)
            }
            EventKind::Get(name, value) => write!(f, "{} = {}", name, value),
        }
    }
}

#[derive(Debug)]
pub struct Command {
    id: u8,
    kind: CommandKind,
}

impl Command {
    pub fn id(&self) -> u8 {
        self.id
    }

    pub fn kind(&self) -> &CommandKind {
        &self.kind
    }
}

#[derive(Debug)]
pub struct Ui {
    print_thread: JoinHandle<()>,
    read_thread: JoinHandle<()>,

    evt_send: Sender<Event>,
    cmd_recv: Receiver<Command>,
}

impl Ui {
    pub fn start(handle: Handle, oob: bool) -> Result<Ui, std::io::Error> {
        let (cmd_send, cmd_recv) = mpsc::channel(1);
        let (evt_send, evt_recv) = mpsc::channel(1);

        let print_handle = handle.clone();
        let print_thread =
            thread::Builder::new()
                .name("ui-print".to_owned())
                .spawn(move || printer(print_handle, evt_recv, oob))?;

        let read_thread = thread::Builder::new()
            .name("ui-read".to_owned())
            .spawn(move || reader(handle, cmd_send))?;

        Ok(Ui {
            print_thread,
            read_thread,
            evt_send,
            cmd_recv,
        })
    }

    pub fn events(&self) -> Events {
        Events(self.evt_send.clone())
    }

    pub async fn recv_command(&mut self) -> Option<Command> {
        self.cmd_recv.recv().await
    }
}

fn printer(handle: Handle, mut events: Receiver<Event>, oob: bool) {
    while let Some(msg) = handle.block_on(events.recv()) {
        if msg.reply_to.is_some() || oob {
            eprint!("\n{}", msg);
        }
    }
}

fn reader(handle: Handle, commands: Sender<Command>) {
    if let Err(e) = try_reader(handle, commands) {
        eprintln!("Reader error: {}", e);
        std::process::abort();
    }
}

fn try_reader(
    handle: Handle,
    mut commands: Sender<Command>,
) -> Result<(), crate::Error> {
    let mut command_id: u8 = 0;
    let mut rl = Editor::<()>::new();

    loop {
        let cid = command_id;
        command_id = command_id.wrapping_add(1);

        let prompt = format!("<{:02X}> ", cid);
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => {
                // TODO: Exit gracefully
                std::process::exit(0);
            }
            Err(e) => return Err(e.into()),
        };

        rl.add_history_entry(&line);

        let parsed = match shell_words::split(&line) {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => continue,
            Err(_) => {
                print!("parse error");
                continue;
            }
        };

        let cmd_kind = match CommandKind::from_iter_safe(parsed) {
            Ok(c) => c,
            Err(e) => {
                print!("\n{}", e);
                continue;
            }
        };

        let cmd = Command {
            id: cid,
            kind: cmd_kind,
        };

        handle.block_on(commands.send(cmd))?;
    }
}
