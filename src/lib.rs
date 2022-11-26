#![allow(non_snake_case)]

mod windows;

use std::time::Duration;

use crate::windows::WindowsSystemMonitor;
use crossbeam_channel::Receiver;

#[derive(Debug, Clone, Copy)]
pub enum SystemEvent {
    DevAdded,
    DevRemoved,
    DevNodesChanged,
}

pub trait Monitor {
    fn into_inner(self) -> Receiver<SystemEvent>;
    fn try_recv(&self) -> Option<SystemEvent>;
    fn recv(&self, timeout: Option<Duration>) -> Option<SystemEvent>;
}

#[cfg(target_os = "windows")]
pub type SystemMonitor = WindowsSystemMonitor;
