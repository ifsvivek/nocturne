//! Storage roots for the server's storage module: where libraries and downloads live. A root is a local folder, a mounted network share (SMB or NFS, mounted by the system or in user space through GVfs) or a folder exported by a `nocturne-server agent` on another machine. Nothing here needs root: shares that cannot be mounted in user space get mount instructions instead.
//!
//! Blocking file-system calls (`fs`, `mounts::mounted`) can hang on a dead network mount; callers run them off the async runtime with a timeout.
pub mod agent;
pub mod fs;
pub mod mounts;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Local,
    Smb,
    Nfs,
    Agent,
}

/// What a root is used for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Library,
    Downloads,
    #[default]
    Both,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Root {
    pub id: u64,
    pub label: String,
    pub kind: Kind,
    pub role: Role,
    /// The folder on this machine (local, smb, nfs). Empty for agent roots.
    pub path: PathBuf,
    /// `smb://host/share[/dir]`, `host:/export` (nfs), `http://host:port` (agent). Empty for local roots.
    pub url: String,
    /// Agent roots: the root's name on the agent.
    pub remote: String,
    /// Local roots: the mount point the folder was on when added (`/` for the root file system). A different one later means the disk is gone.
    pub mount_point: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Space {
    pub free: u64,
    pub total: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    /// The folder (or the agent) answers.
    pub reachable: bool,
    pub writable: bool,
    /// The share (or the disk) the root lives on is mounted. Always true for agent roots that answer.
    pub mounted: bool,
    pub space: Option<Space>,
    pub error: Option<String>,
}

/// One file or folder (agent listings and the browse route).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Unix seconds.
    pub modified: u64,
}
