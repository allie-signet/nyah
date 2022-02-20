use hex::FromHex;
use laminar::{
    Config as LaminarConfig, Socket as LaminarSocket,
};
use nyah::state::*;

use std::env;
use std::error::Error;

use std::net::UdpSocket;
use std::time::{Instant, Duration};

fn main() -> Result<(), Box<dyn Error>> {
    let file = env::args().nth(1).unwrap();
    let hash = <[u8; 24]>::from_hex(env::args().nth(2).unwrap()).unwrap();
    let host = env::args().nth(3).unwrap();
    // let conn = env::args().nth(4).unwrap();

    // let socket = UdpSocket::bind("127.0.0.1:25565")?;
    let socket = UdpSocket::bind(host)?;
    socket.set_broadcast(true)?;

    let local_ip = local_ip_address::local_ip().unwrap();

    let mut socket = LaminarSocket::bind_internal(socket, LaminarConfig::default())?;
    let event_receiver = socket.get_event_receiver();
    let event_sender = socket.get_packet_sender();

    let _thread = std::thread::spawn(move || socket.start_polling());

    let mut state = NyahState::new(event_sender, Some(local_ip));
    state.add_desired_box(hash, file);

    let mut last_peer_search = Instant::now();

    loop {
        if (state.peers.is_empty() && last_peer_search.elapsed() > Duration::from_secs(20)) || last_peer_search.elapsed() > Duration::from_secs(20) {
            state.search_for_peers("255.255.255.255:25565".parse().unwrap())?;
            last_peer_search = Instant::now();
        }

        if let Ok(event) = event_receiver.recv_timeout(Duration::from_millis(400)) {
            state.handle_packet(event)?;
        } else {
            state.search_for_metadata()?;
            state.search_for_pieces()?;
        }
    }
}