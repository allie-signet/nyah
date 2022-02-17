use nyah::messages::*;
use nyah::state::*;
use std::env;
use std::io;
use std::net::UdpSocket;

fn main() -> io::Result<()> {
    let file = env::args().nth(1).unwrap();
    let host = env::args().nth(2).unwrap();

    let mut state = State::default();
    let hash = state.add_whole_file(file)?;
    println!("{}", state.files.len());
    println!("{}", state.files[&hash].pieces.len());
    println!("{}", hex::encode(hash));
    let local_ip = local_ip_address::local_ip().unwrap();

    let socket = UdpSocket::bind(host)?;
    socket.set_broadcast(true)?;

    loop {
        let msg = socket.read_message(local_ip)?;
        println!("{:?}", msg);

        use Message::*;
        match msg {
            SearchingForPeers(addr) => {
                println!("peer asked for ack; sending");
                socket.ack_peer_search(addr)?;
            }
            ImHere(addr) => {
                println!("adding peer");
                state.peers.insert(addr);
            }
            FindMetadata { from, key } => {
                if let Some(ref metadata) = state.get_metadata(&key) {
                    println!("peer asked for metadata; sending");
                    socket.send_metadata(&key, metadata, from)?;
                }
            }
            GotMetadata {
                from: _,
                key: _,
                metadata: _,
            } => {
                println!("got metadata!");
            }
            Message::FindPiece { from, key, piece } => {
                println!("got ask for piece, sending");
                socket.ack_piece(&key, piece, from)?;
            }
            Message::StartDownload { from, key, piece } => {
                if let Some(data) = state.read_piece(&key, piece) {
                    for (i, ch) in data.chunks(256).enumerate() {
                        // lol. lmao
                        // lol. lmao
                        for _ in 0..4 {
                            socket.upload(&key, piece, ch, i as u32, from)?;
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(())
}
