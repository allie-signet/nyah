use crate::*;

use laminar::Packet as LaminarPacket;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct BoxState {
    pub name: String,
    pub box_hash: BoxHash,
    pub files: Vec<FileState>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileState {
    pub path: PathBuf,
    pub pieces_downloaded: usize,
    pub total_pieces: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum IPCResponse {
    Ok,
    NotFound,
    BoxCreated(BoxHash),
    Peers(Vec<SocketAddr>),
    Box(BoxState),
    Boxes(Vec<BoxState>),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum IPCCall {
    CreateBox(String, PathBuf),
    DownloadBox(BoxHash, PathBuf),
    GetBoxState(BoxHash),
    GetAllPeers,
    GetAllBoxes,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Message {
    SearchingForPeers,
    ImHere,
    FindMetadata(BoxHash),
    GotMetadata(BoxHash, CardboardMetadata),
    FindPiece {
        id: BoxHash,
        file_index: usize,
        piece_index: usize,
    },
    GotPiece {
        id: BoxHash,
        file_index: usize,
        piece_index: usize,
    },
    StartDownload {
        id: BoxHash,
        file_index: usize,
        piece_index: usize,
    },
    Upload {
        id: BoxHash,
        file_index: usize,
        piece_index: usize,
        chunk_index: usize,
        buf: Vec<u8>,
    },
}

impl Message {
    pub fn to_packet(&self, dest: SocketAddr) -> LaminarPacket {
        LaminarPacket::reliable_unordered(dest, rmp_serde::to_vec(self).unwrap())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CardboardMetadata {
    pub name: String,
    pub files: Vec<FileMetadata>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub size: usize,
    pub pieces: Vec<[u8; 16]>,
}
