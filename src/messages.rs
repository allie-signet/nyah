use crate::state::FileMetadata;
use crate::*;

use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};

#[derive(Debug)]
pub enum Message {
    SearchingForPeers(SocketAddr),
    ImHere(SocketAddr),
    FindMetadata {
        from: SocketAddr,
        key: FileHash,
    },
    GotMetadata {
        from: SocketAddr,
        key: FileHash,
        metadata: FileMetadata,
    },
    FindPiece {
        from: SocketAddr,
        key: FileHash,
        piece: u32,
    },
    GotPiece {
        from: SocketAddr,
        key: FileHash,
        piece: u32,
    },
    StartDownload {
        from: SocketAddr,
        key: FileHash,
        piece: u32,
    },
    Upload {
        from: SocketAddr,
        key: FileHash,
        piece: u32,
        idx: u32,
        buf: Box<[u8]>,
    },
}

pub trait NyahSocket {
    fn read_message(&self, ip: IpAddr) -> io::Result<Message>;

    fn send_peer_search(&self, addr: impl ToSocketAddrs) -> io::Result<usize>;

    fn ack_peer_search(&self, addr: impl ToSocketAddrs) -> io::Result<usize>;

    fn ask_for_metadata(&self, key: &[u8], addr: impl ToSocketAddrs) -> io::Result<usize>;

    fn send_metadata(
        &self,
        key: &[u8],
        metadata: &[u8],
        addr: impl ToSocketAddrs,
    ) -> io::Result<usize>;

    fn search_for_piece(
        &self,
        key: &[u8],
        piece: u32,
        addr: impl ToSocketAddrs,
    ) -> io::Result<usize>;

    fn ack_piece(&self, key: &[u8], piece: u32, addr: impl ToSocketAddrs) -> io::Result<usize>;

    fn start_download(&self, key: &[u8], piece: u32, addr: impl ToSocketAddrs)
        -> io::Result<usize>;

    fn upload(
        &self,
        key: &[u8],
        piece: u32,
        data: &[u8],
        idx: u32,
        addr: impl ToSocketAddrs,
    ) -> io::Result<usize>;
}

impl NyahSocket for UdpSocket {
    fn read_message(&self, local_ip: IpAddr) -> io::Result<Message> {
        let mut peek_buf: [u8; 1] = [0; 1];
        let (_, nya) = self.peek_from(&mut peek_buf)?;
        println!("{}", peek_buf[0]);
        println!("{}", nya);
        if nya.ip() == local_ip {
            self.recv_from(&mut peek_buf)?;
            println!("filtering packet from self");
            return Err(std::io::ErrorKind::WouldBlock.into());
        }

        match peek_buf[0] {
            0 => {
                let (_, addr) = self.recv_from(&mut peek_buf)?;
                Ok(Message::SearchingForPeers(addr))
            }
            1 => {
                let (_, addr) = self.recv_from(&mut peek_buf)?;
                Ok(Message::ImHere(addr))
            }
            2 => {
                let mut buf: [u8; 25] = [0; 25];
                let (_, from) = self.recv_from(&mut buf)?;
                Ok(Message::FindMetadata {
                    from,
                    key: buf[1..].try_into().unwrap(),
                })
            }
            3 => {
                let mut buf: [u8; 65484] = [0; 65484];
                let (bytes_read, from) = self.recv_from(&mut buf)?;
                let key: FileHash = buf[1..25].try_into().unwrap();
                let metadata = FileMetadata::from_bytes(&buf[25..bytes_read])?;

                Ok(Message::GotMetadata {
                    from,
                    key,
                    metadata,
                })
            }
            4 => {
                let mut buf: [u8; 29] = [0; 29];
                let (_, from) = self.recv_from(&mut buf)?;

                Ok(Message::FindPiece {
                    from,
                    key: buf[1..25].try_into().unwrap(),
                    piece: u32::from_le_bytes(buf[25..].try_into().unwrap()),
                })
            }
            5 => {
                let mut buf: [u8; 29] = [0; 29];
                let (_, from) = self.recv_from(&mut buf)?;

                Ok(Message::GotPiece {
                    from,
                    key: buf[1..25].try_into().unwrap(),
                    piece: u32::from_le_bytes(buf[25..].try_into().unwrap()),
                })
            }
            6 => {
                let mut buf: [u8; 29] = [0; 29];
                let (_, from) = self.recv_from(&mut buf)?;

                Ok(Message::StartDownload {
                    from,
                    key: buf[1..25].try_into().unwrap(),
                    piece: u32::from_le_bytes(buf[25..].try_into().unwrap()),
                })
            }
            7 => {
                let mut buf: [u8; 48_033] = [0; 48_033];
                let (bytes_read, from) = self.recv_from(&mut buf)?;

                Ok(Message::Upload {
                    from,
                    key: buf[1..25].try_into().unwrap(),
                    piece: u32::from_le_bytes(buf[25..29].try_into().unwrap()),
                    idx: u32::from_le_bytes(buf[29..33].try_into().unwrap()),
                    buf: Box::from(&buf[33..bytes_read]),
                })
            }
            _ => unimplemented!(),
        }
    }

    fn send_peer_search(&self, addr: impl ToSocketAddrs) -> io::Result<usize> {
        self.send_to(&[0], addr)
    }

    fn ack_peer_search(&self, addr: impl ToSocketAddrs) -> io::Result<usize> {
        self.send_to(&[1], addr)
    }

    fn ask_for_metadata(&self, key: &[u8], addr: impl ToSocketAddrs) -> io::Result<usize> {
        self.send_to(&[&[2], key].concat(), addr)
    }

    fn send_metadata(
        &self,
        key: &[u8],
        metadata: &[u8],
        addr: impl ToSocketAddrs,
    ) -> io::Result<usize> {
        self.send_to(&[&[3], key, metadata].concat(), addr)
    }

    fn search_for_piece(
        &self,
        key: &[u8],
        piece: u32,
        addr: impl ToSocketAddrs,
    ) -> io::Result<usize> {
        self.send_to(&[&[4], key, &piece.to_le_bytes()].concat(), addr)
    }

    fn ack_piece(&self, key: &[u8], piece: u32, addr: impl ToSocketAddrs) -> io::Result<usize> {
        self.send_to(&[&[5], key, &piece.to_le_bytes()].concat(), addr)
    }

    fn start_download(
        &self,
        key: &[u8],
        piece: u32,
        addr: impl ToSocketAddrs,
    ) -> io::Result<usize> {
        self.send_to(&[&[6], key, &piece.to_le_bytes()].concat(), addr)
    }

    fn upload(
        &self,
        key: &[u8],
        piece: u32,
        data: &[u8],
        idx: u32,
        addr: impl ToSocketAddrs,
    ) -> io::Result<usize> {
        self.send_to(
            &[&[7], key, &piece.to_le_bytes(), &idx.to_le_bytes(), data].concat(),
            addr,
        )
    }
}
