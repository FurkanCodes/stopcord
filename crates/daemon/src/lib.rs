//! The Sawtooth daemon: it owns every terminal, keeps them running when no
//! window is open, tells what each agent is doing, and answers on a local socket.
//!
//! The socket speaks newline-delimited JSON. Send `{"id":"1","method":"…","params":{…}}`,
//! get `{"id":"1","result":…}` or `{"id":"1","error":{"code":"…","message":"…"}}`.
//! `events.subscribe` keeps the connection open and pushes `{"event":"…",…}` lines.
//! `docs/terminal-daemon.md` lists every method.

pub mod client;
pub mod confine;
pub mod identify;
pub mod paths;
pub mod server;
pub mod tracker;
pub mod viewer;

pub use client::Client;
pub use server::{Config, Daemon};
