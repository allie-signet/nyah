// pub mod messages;
// pub mod state;
pub mod file;
pub use types::*;
pub mod state;
pub mod types;

use blake2::{digest::consts::U16, Blake2s, Digest};

pub const PIECE_SIZE: usize = 48_000;
pub const CHUNK_SIZE: usize = 256;

pub type PieceHash = [u8; 16];
pub type BoxHash = [u8; 16];
pub type FileHash = [u8; 16];
pub type Blake2s16 = Blake2s<U16>;
