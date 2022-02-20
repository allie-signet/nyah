use laminar::{
    Config as LaminarConfig, Socket as LaminarSocket,
};
use nyah::state::*;

use std::env;
use std::error::Error;

use std::net::UdpSocket;


fn main() -> Result<(), Box<dyn Error>> {
    let file = env::args().nth(1).unwrap();
    let host = env::args().nth(2).unwrap();

    let local_ip = local_ip_address::local_ip().unwrap();

    let socket = UdpSocket::bind(host)?;
    socket.set_broadcast(true)?;

    let mut socket = LaminarSocket::bind_internal(socket, LaminarConfig::default())?;

    let event_receiver = socket.get_event_receiver();
    let event_sender = socket.get_packet_sender();

    let _thread = std::thread::spawn(move || socket.start_polling());

    let mut state = NyahState::new(event_sender, Some(local_ip));
    println!("{}", hex::encode(state.create_box("owo".to_owned(), file)?));
    
    for ev in event_receiver.iter() {
        state.handle_packet(ev)?;
    }

    Ok(())
}
