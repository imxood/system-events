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
    fn recv_ref(&self) -> &Receiver<SystemEvent>;
}

#[cfg(target_os = "windows")]
pub type SystemMonitor = WindowsSystemMonitor;

pub struct EventNotify<T> {
    notify: Box<dyn Fn(T)>,
}

impl<T> EventNotify<T> {
    pub fn new(notify: Box<dyn Fn(T)>) -> Self {
        Self { notify }
    }

    pub fn exec(&self, param: T) {
        (self.notify)(param);
    }
}

unsafe impl<T> Send for EventNotify<T> {}
