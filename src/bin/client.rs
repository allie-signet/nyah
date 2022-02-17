use hex::FromHex;
use nyah::messages::*;
use nyah::state::*;
use std::collections::HashMap;
use std::env;
use std::io;
use std::net::UdpSocket;

enum PieceState {
    Complete,
    Downloading(usize, Vec<u32>),
}

fn main() -> io::Result<()> {
    let file = env::args().nth(1).unwrap();
    let hash = <[u8; 24]>::from_hex(env::args().nth(2).unwrap()).unwrap();
    let host = env::args().nth(3).unwrap();
    // let conn = env::args().nth(4).unwrap();

    let mut state = State::default();
    // let socket = UdpSocket::bind("127.0.0.1:25565")?;
    let socket = UdpSocket::bind(host)?;
    socket.set_read_timeout(Some(std::time::Duration::from_millis(3000)))?;
    socket.set_broadcast(true)?;

    let local_ip = local_ip_address::local_ip().unwrap();

    println!("{}", socket.local_addr()?);

    socket.send_peer_search("255.255.255.255:25565")?;

    let mut pieces: HashMap<u32, PieceState> = HashMap::new();

    loop {
        let msg = match socket.read_message(local_ip) {
            Ok(v) => v,
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    if state.files.contains_key(&hash) {
                        // for (i, piece) in state.files[&hash].pieces.iter().enumerate() {
                        //     if !pieces.contains_key(&(i as u32)) {
                        //         for peer in &state.peers {
                        //             socket.start_download(&hash, i as u32, peer)?;
                        //         }

                        //         pieces.insert(
                        //             i as u32,
                        //             PieceState::Downloading(
                        //                 piece.size / 400,
                        //                 Vec::with_capacity(piece.size / 400),
                        //             ),
                        //         );
                        //     }
                        // }
                    } else {
                        for peer in &state.peers {
                            socket.ask_for_metadata(&hash, peer)?;
                        }
                    }

                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        use Message::*;
        match msg {
            SearchingForPeers(addr) => {
                println!("peer asked for ack; sending");
                socket.ack_peer_search(addr)?;
            }
            ImHere(addr) => {
                println!("adding peer");
                state.peers.insert(addr);
                socket.ask_for_metadata(&hash, addr)?;
            }
            FindMetadata { from, key } => {
                println!("peer asked for metadata; sending");
                if let Some(ref metadata) = state.get_metadata(&key) {
                    socket.send_metadata(&key, metadata, from)?;
                }
            }
            GotMetadata {
                from: _,
                key,
                metadata,
            } => {
                state.create_or_add_file(&file, key, metadata)?;
                for (i, _) in state.files[&key].pieces.iter().enumerate() {
                    for peer in &state.peers {
                        socket.search_for_piece(&key, i as u32, peer)?;
                    }
                }
                println!("got metadata!");
            }
            GotPiece { from, key, piece } => {
                println!("got piece {}, asking for dl", piece);
                pieces.insert(
                    piece as u32,
                    PieceState::Downloading(
                        state.files[&hash].pieces[piece as usize].size / 256,
                        Vec::with_capacity(state.files[&hash].pieces[piece as usize].size / 256),
                    ),
                );
                socket.start_download(&key, piece, from)?;
            }
            Upload {
                from,
                key,
                piece,
                idx,
                buf,
            } => {
                if let Some(piece_state) = pieces.get_mut(&piece) {
                    if let PieceState::Downloading(total, acquired) = piece_state {
                        println!(
                            "got piece data for piece {} index {} ({}/{} acquired), writing",
                            piece,
                            idx,
                            acquired.len(),
                            total
                        );
                        println!("data len: {}", buf.len());

                        if !acquired.contains(&idx) {
                            if state.write_partial_piece(&key, piece, idx * 256, &buf) {
                                println!("write sucess!");
                                acquired.push(idx);
                            } else {
                                println!("write fail >:");
                            }
                        }

                        if *total == acquired.len() {
                            if state.verify_piece(&key, piece) {
                                println!("piece {} verify sucess", piece);
                                println!(
                                    "downloaded pieces: {}/{}",
                                    state.files[&key]
                                        .pieces
                                        .iter()
                                        .filter(|v| v.downloaded)
                                        .count(),
                                    state.files[&key].pieces.len()
                                );
                                state.files[&key].inner.flush()?;
                                *piece_state = PieceState::Complete;
                            } else {
                                println!("piece {} verify failure", piece);
                                acquired.clear();
                                socket.start_download(&key, piece, from)?;
                            }
                        }
                    }
                }
            }
            _ => panic!(),
        }
    }

    Ok(())
}
