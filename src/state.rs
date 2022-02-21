use crate::file::*;

use laminar::{Packet as LaminarPacket, SocketEvent};
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use crossbeam_channel::Sender;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

pub struct NyahState {
    pub boxes: BTreeMap<BoxHash, CardboardBox>,
    pub peers: HashSet<SocketAddr>,
    packet_sender: Sender<LaminarPacket>,
    pub looking_for_boxes: BTreeMap<BoxHash, PathBuf>,
    filter_from: Option<IpAddr>, // filter events from this address
}

impl NyahState {
    pub fn new(sender: Sender<LaminarPacket>, filter_from: Option<IpAddr>) -> NyahState {
        NyahState {
            packet_sender: sender,
            peers: HashSet::new(),
            boxes: BTreeMap::new(),
            looking_for_boxes: BTreeMap::new(),
            filter_from,
        }
    }

    pub fn get_metadata(&self, key: BoxHash) -> Option<CardboardMetadata> {
        self.boxes.get(&key).map(|v| v.metadata.clone())
    }

    pub fn create_box(
        &mut self,
        box_name: String,
        box_dir: impl AsRef<Path>,
    ) -> io::Result<BoxHash> {
        let cardboard_box = CardboardBox::create(box_name, box_dir)?;
        let hash = cardboard_box.hash;
        self.boxes.insert(hash, cardboard_box);

        Ok(hash)
    }

    pub fn add_box(
        &mut self,
        box_dir: impl AsRef<Path>,
        hash: BoxHash,
        metadata: CardboardMetadata,
    ) -> io::Result<()> {
        let cardboard_box = CardboardBox::from_metadata(box_dir, hash, metadata)?;
        let hash = cardboard_box.hash;
        self.boxes.insert(hash, cardboard_box);

        Ok(())
    }

    pub fn add_desired_box(&mut self, box_hash: BoxHash, box_dir: impl AsRef<Path>) {
        self.looking_for_boxes
            .insert(box_hash, box_dir.as_ref().to_owned());
    }

    pub fn handle_packet(&mut self, event: SocketEvent) -> io::Result<()> {
        match event {
            SocketEvent::Packet(p) => {
                if self
                    .filter_from
                    .as_ref()
                    .map(|ip| ip == &p.addr().ip())
                    .unwrap_or(false)
                {
                    return Ok(());
                }

                self.handle_msg(p.addr(), rmp_serde::from_read_ref(p.payload()).unwrap())?;
            }
            // SocketEvent::Disconnect(p) => {
            //     self.peers.remove(&p);
            // }
            _ => (),
        }

        Ok(())
    }

    pub fn handle_msg(&mut self, from: SocketAddr, message: Message) -> io::Result<()> {
        use Message::*;

        match message {
            SearchingForPeers => {
                self.peers.insert(from);
                self.send_packet(ImHere.to_packet(from))?
            }
            ImHere => {
                self.peers.insert(from);
            }
            FindMetadata(hash) => {
                if let Some(metadata) = self.get_metadata(hash) {
                    self.send_packet(GotMetadata(hash, metadata).to_packet(from))?;
                }
            }
            GotMetadata(hash, metadata) => {
                if let Some(path) = self.looking_for_boxes.remove(&hash) {
                    self.add_box(path, hash, metadata)?;
                }
            }
            FindPiece {
                id,
                file_index,
                piece_index,
            } => {
                if let Some(file) = self.boxes.get(&id).and_then(|b| b.files.get(file_index)) {
                    if file.has_piece(piece_index) {
                        self.send_packet(
                            GotPiece {
                                id,
                                file_index,
                                piece_index,
                            }
                            .to_packet(from),
                        )?;
                    }
                }
            }
            GotPiece {
                id,
                file_index,
                piece_index,
            } => {
                if let Some(file) = self.boxes.get(&id).and_then(|b| b.files.get(file_index)) {
                    if !file.has_piece(piece_index) {
                        self.send_packet(
                            StartDownload {
                                id,
                                file_index,
                                piece_index,
                            }
                            .to_packet(from),
                        )?;
                    }
                }
            }
            StartDownload {
                id,
                file_index,
                piece_index,
            } => {
                if let Some(data) = self
                    .boxes
                    .get(&id)
                    .and_then(|b| b.files.get(file_index))
                    .and_then(|f| f.read_piece(piece_index))
                {
                    for (i, ch) in data.chunks(CHUNK_SIZE).enumerate() {
                        std::thread::sleep(Duration::from_millis(30)); // opencomputers can get overwhelmed if we go too fast here

                        self.send_packet(
                            Upload {
                                id,
                                file_index,
                                piece_index,
                                chunk_index: i,
                                buf: ch.to_vec(),
                            }
                            .to_packet(from),
                        )?;
                    }
                }
            }
            Upload {
                id,
                file_index,
                piece_index,
                chunk_index,
                buf,
            } => {
                if let Some(file) = self.boxes.get(&id).and_then(|b| b.files.get(file_index)) {
                    if !file.has_piece(piece_index) {
                        file.write_chunk(piece_index, chunk_index, &buf);
                    }
                }
            }
        }

        Ok(())
    }

    pub fn search_for_metadata(&self) -> io::Result<()> {
        for k in self.looking_for_boxes.keys() {
            for peer in &self.peers {
                self.send_packet(Message::FindMetadata(*k).to_packet(*peer))?;
            }
        }

        Ok(())
    }

    pub fn search_for_pieces(&self) -> io::Result<()> {
        for b in self.boxes.values() {
            for (file_index, piece_indexes) in b.needed_pieces() {
                for piece_index in piece_indexes {
                    for peer in &self.peers {
                        self.send_packet(
                            Message::FindPiece {
                                id: b.hash,
                                file_index,
                                piece_index,
                            }
                            .to_packet(*peer),
                        )?;
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn search_for_peers(&self, addr: SocketAddr) -> io::Result<()> {
        self.send_packet(LaminarPacket::unreliable(
            addr,
            rmp_serde::to_vec(&Message::SearchingForPeers).unwrap(),
        ))
    }

    fn send_packet(&self, packet: LaminarPacket) -> io::Result<()> {
        self.packet_sender
            .send(packet)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}
