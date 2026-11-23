use std::collections::HashSet;
use std::sync::{Mutex as StdMutex, OnceLock};

mod acp;
mod opencode;
mod process;

pub use acp::AcpEngine;
pub use opencode::OpenCodeEngine;

static ACTIVE_CHILD_PGIDS: OnceLock<StdMutex<HashSet<i32>>> = OnceLock::new();

fn active_pgids() -> &'static StdMutex<HashSet<i32>> {
    ACTIVE_CHILD_PGIDS.get_or_init(|| StdMutex::new(HashSet::new()))
}

pub fn register_child_pgid(pgid: i32) {
    if pgid > 0 {
        if let Ok(mut set) = active_pgids().lock() {
            set.insert(pgid);
        }
    }
}

pub fn unregister_child_pgid(pgid: i32) {
    if pgid > 0 {
        if let Ok(mut set) = active_pgids().lock() {
            set.remove(&pgid);
        }
    }
}

pub fn kill_registered_subprocesses() {
    if let Ok(set) = active_pgids().lock() {
        for &pgid in set.iter() {
            if pgid > 0 {
                unsafe {
                    libc::killpg(pgid, libc::SIGTERM);
                }
            }
        }
    }
}
