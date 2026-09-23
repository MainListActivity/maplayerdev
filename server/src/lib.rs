pub mod config;
pub mod discovery;
pub mod net;
pub mod profiles;
pub mod proto;
pub mod session;

pub const ALPN: &[u8] = b"maplayer/1";
