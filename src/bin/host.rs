use laminar::{Config as LaminarConfig, Socket as LaminarSocket};
use nyah::state::*;
use nyah::*;
use std::error::Error;

use std::fs;
use std::io::Write;
use std::net::{Shutdown, UdpSocket};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let local_ip = local_ip_address::local_ip().unwrap();

    let socket = UdpSocket::bind("0.0.0.0:25565")?;
    socket.set_broadcast(true)?;

    let mut socket = LaminarSocket::bind_internal(socket, LaminarConfig::default())?;

    let event_receiver = socket.get_event_receiver();
    let event_sender = socket.get_packet_sender();

    let _thread = std::thread::spawn(move || socket.start_polling());

    let mut state = NyahState::new(event_sender, Some(local_ip));

    fs::remove_file("/var/run/nyah.sock");
    let ipc_socket = UnixListener::bind("/var/run/nyah.sock")?;
    ipc_socket.set_nonblocking(true)?;

    let mut last_peer_search = Instant::now();

    loop {
        if let Ok((mut peer, _)) = ipc_socket.accept() {
            use IPCCall::*;

            let call: IPCCall = rmp_serde::from_read(&mut peer).unwrap();
            let res = match call {
                CreateBox(name, path) => {
                    let hash = state.create_box(name, path)?;
                    IPCResponse::BoxCreated(hash)
                }
                DownloadBox(hash, path) => {
                    state.add_desired_box(hash, path);
                    IPCResponse::Ok
                }
                GetBoxState(hash) => {
                    if let Some(s) = state.boxes.get(&hash).map(|b| b.get_download_state()) {
                        IPCResponse::Box(s)
                    } else {
                        IPCResponse::NotFound
                    }
                }
                GetAllPeers => IPCResponse::Peers(state.peers.iter().copied().collect()),
                GetAllBoxes => IPCResponse::Boxes(
                    state
                        .boxes
                        .values()
                        .map(|b| b.get_download_state())
                        .collect(),
                ),
            };

            rmp_serde::encode::write(&mut peer, &res).unwrap();
            peer.flush()?;
            peer.shutdown(Shutdown::Both)?;
        }

        if (state.peers.is_empty() && last_peer_search.elapsed() > Duration::from_secs(20))
            || last_peer_search.elapsed() > Duration::from_secs(20)
        {
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

    Ok(())
}
