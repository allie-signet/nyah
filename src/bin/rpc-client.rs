use argh::FromArgs;
use nyah::*;
use std::fs;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use libhumancode::{decode_chunk, encode_chunk};
const HASH_ECC_SYMBOLS: u8 = 5;
const HASH_BITS: u8 = 128;

#[derive(FromArgs, PartialEq, Debug)]
/// control the current nyah instance
struct RPCArgs {
    #[argh(subcommand)]
    cmd: SubCommand,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum SubCommand {
    CreateBox(CreateBoxCmd),
    DownloadBox(DownloadBoxCmd),
    GetBoxState(GetBoxStateCmd),
    GetAllBoxes(GetAllBoxesCmd),
    GetAllPeers(GetAllPeersCmd),
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "create")]
/// creates a new box, returning it's hash.
struct CreateBoxCmd {
    #[argh(positional)]
    /// the box name.
    name: String,
    #[argh(positional)]
    /// the place to look for the box's files
    path: PathBuf,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "download")]
/// downloads a box to the specified path.
struct DownloadBoxCmd {
    #[argh(positional)]
    hash: String,
    #[argh(positional)]
    path: PathBuf,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "details")]
/// gets the download state of a box.
struct GetBoxStateCmd {
    #[argh(positional)]
    hash: String,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "list-peers")]
/// gets all peers known to Nyah.
struct GetAllPeersCmd {}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "status")]
/// gets all boxes currently being downloaded or seeded.
struct GetAllBoxesCmd {
    #[argh(switch)]
    /// display boxes verbosely (with per-file details) or not
    verbose: bool,
}

fn main() -> io::Result<()> {
    let RPCArgs { cmd } = argh::from_env();
    use SubCommand::*;
    match cmd {
        CreateBox(CreateBoxCmd { name, path }) => {
            let path = fs::canonicalize(path)?;

            if let Ok(IPCResponse::BoxCreated(hash)) = call(IPCCall::CreateBox(name, path)) {
                println!(
                    "created box! here's it's hash: {}",
                    encode_chunk(&hash, HASH_ECC_SYMBOLS, HASH_BITS)
                        .unwrap()
                        .pretty()
                        .as_str()
                );
            } else {
                println!("couldn't create box >:");
            }
        }
        DownloadBox(DownloadBoxCmd { hash, path }) => {
            fs::create_dir_all(&path)?;
            let path = fs::canonicalize(&path)?;
            println!("{:?}", path);

            let (decoded, _corrected) = decode_chunk(&hash, HASH_ECC_SYMBOLS, HASH_BITS)
                .expect("weird! i couldn't decode the hash you gave me.");

            if let Ok(IPCResponse::Ok) = call(IPCCall::DownloadBox(
                decoded.as_bytes().try_into().unwrap(),
                path,
            )) {
                println!("downloading box!");
            } else {
                println!("couldn't add box >:");
            }
        }
        GetBoxState(GetBoxStateCmd { hash }) => {
            let (decoded, _corrected) = decode_chunk(&hash, HASH_ECC_SYMBOLS, HASH_BITS)
                .expect("weird! i couldn't decode the hash you gave me.");

            match call(IPCCall::GetBoxState(decoded.as_bytes().try_into().unwrap()))? {
                IPCResponse::Box(state) => display_box_verbose(state),
                IPCResponse::NotFound => println!("box not found - if you recently added it, we may not have metadata for it yet!"),
                _ => unreachable!()
            }
        }
        GetAllBoxes(GetAllBoxesCmd { verbose }) => {
            if let IPCResponse::Boxes(states) = call(IPCCall::GetAllBoxes)? {
                if verbose {
                    for s in states {
                        display_box_verbose(s);
                        println!("")
                    }
                } else {
                    for s in states {
                        display_box_min(s)
                    }
                }
            }
        }
        GetAllPeers(_) => {
            if let IPCResponse::Peers(peers) = call(IPCCall::GetAllPeers)? {
                println!("current peers:");
                for peer in peers {
                    println!("> {}", peer);
                }
            }
        }
    }

    Ok(())
}

fn call(msg: IPCCall) -> io::Result<IPCResponse> {
    let mut stream = UnixStream::connect("/var/run/nyah.sock")?;
    rmp_serde::encode::write(&mut stream, &msg).unwrap();

    Ok(rmp_serde::from_read(&mut stream).unwrap())
}

fn display_box_verbose(state: BoxState) {
    println!(
        "cat box {}\n(hash {})",
        state.name,
        encode_chunk(&state.box_hash, HASH_ECC_SYMBOLS, HASH_BITS)
            .unwrap()
            .pretty()
            .as_str()
    );

    for entry in state.files {
        println!(
            "> {} - {}% done ({}/{} pieces)",
            entry.path.into_os_string().to_str().unwrap(),
            (entry.pieces_downloaded * 100) / entry.total_pieces,
            entry.pieces_downloaded,
            entry.total_pieces
        );
    }
}

fn display_box_min(state: BoxState) {
    let (total, done) = state.files.iter().fold((0, 0), |(total, downloaded), f| {
        (total + f.total_pieces, downloaded + f.pieces_downloaded)
    });

    println!(
        "cat box {}\n(hash {})\n> {}% done ({}/{} pieces)",
        state.name,
        encode_chunk(&state.box_hash, HASH_ECC_SYMBOLS, HASH_BITS)
            .unwrap()
            .pretty()
            .as_str(),
        (done * 100) / total,
        done,
        total
    );
}
