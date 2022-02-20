// pub use crate::messages::*;
pub use crate::*;

use memmap2::MmapMut;
use std::cell::RefCell;

use std::fs::{self, File, OpenOptions};
use std::io;

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(PartialEq, Debug)]
pub enum PieceState {
    Downloaded,
    Incomplete(usize, Vec<usize>),
}

#[derive(Debug)]
pub struct Piece {
    pub hash: [u8; 16],
    pub size: usize,
    pub state: RefCell<PieceState>,
    pub pointer: RefCell<*const u8>,
}

#[derive(Debug)]
pub struct MappedFile {
    pub inner: MmapMut,
    pub size: usize,
    pub pieces: Vec<Piece>,
}

impl MappedFile {
    pub fn from_whole_file(f: &File) -> io::Result<(FileHash, MappedFile)> {
        let mut file_hasher = Blake2s24::new();
        let mut piece_hasher = Blake2s16::new();
        let mmap = unsafe { MmapMut::map_mut(f)? };
        let mut piece_pointers = Vec::new();

        for piece in mmap.chunks(PIECE_SIZE) {
            file_hasher.update(&piece);
            piece_hasher.update(&piece);

            piece_pointers.push(Piece {
                hash: piece_hasher.finalize_reset().into(),
                state: RefCell::new(PieceState::Downloaded),
                size: piece.len(),
                pointer: RefCell::new(piece.as_ptr()),
            });
        }

        Ok((
            file_hasher.finalize().into(),
            MappedFile {
                inner: mmap,
                size: f.metadata()?.len() as usize,
                pieces: piece_pointers,
            },
        ))
    }

    pub fn from_file_verified(f: &File, piece_hashes: &[[u8; 16]]) -> io::Result<MappedFile> {
        let mmap = unsafe { MmapMut::map_mut(f)? };
        let piece_iter = mmap.chunks(PIECE_SIZE);

        if piece_hashes.len() != piece_iter.len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let mut piece_hasher = Blake2s16::new();
        let mut piece_pointers = Vec::new();

        for (i, piece) in piece_iter.enumerate() {
            piece_hasher.update(&piece);

            let hash = piece_hasher.finalize_reset().into();

            let chunk_amt = (piece.len() as f64 / CHUNK_SIZE as f64).ceil() as usize;

            piece_pointers.push(Piece {
                hash,
                state: RefCell::new(if piece_hashes[i] == hash {
                    PieceState::Downloaded
                } else {
                    PieceState::Incomplete(chunk_amt, Vec::with_capacity(chunk_amt))
                }),
                size: piece.len(),
                pointer: RefCell::new(piece.as_ptr()),
            });
        }

        Ok(MappedFile {
            inner: mmap,
            size: f.metadata()?.len() as usize,
            pieces: piece_pointers,
        })
    }

    pub fn from_file_empty(f: &File, piece_hashes: &[[u8; 16]]) -> io::Result<MappedFile> {
        let mmap = unsafe { MmapMut::map_mut(f)? };
        let piece_iter = mmap.chunks(PIECE_SIZE);

        if piece_hashes.len() != piece_iter.len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let mut piece_pointers = Vec::new();

        for (i, piece) in piece_iter.enumerate() {
            let chunk_amt = (piece.len() as f64 / CHUNK_SIZE as f64).ceil() as usize;

            piece_pointers.push(Piece {
                hash: piece_hashes[i],
                state: RefCell::new(PieceState::Incomplete(
                    chunk_amt,
                    Vec::with_capacity(chunk_amt),
                )),
                size: piece.len(),
                pointer: RefCell::new(piece.as_ptr()),
            });
        }

        Ok(MappedFile {
            inner: mmap,
            size: f.metadata()?.len() as usize,
            pieces: piece_pointers,
        })
    }

    pub fn has_piece(&self, piece: usize) -> bool {
        if let Some(piece) = self.pieces.get(piece as usize) {
            *piece.state.borrow() == PieceState::Downloaded
        } else {
            false
        }
    }

    pub fn read_piece(&self, piece: usize) -> Option<&[u8]> {
        if let Some(piece) = self.pieces.get(piece as usize) {
            if *piece.state.borrow() == PieceState::Downloaded {
                return Some(unsafe {
                    std::slice::from_raw_parts(*piece.pointer.borrow(), piece.size)
                });
            }
        }

        None
    }

    pub fn verify_piece(&self, piece: usize) -> bool {
        if let Some(data) = self.read_piece(piece) {
            Blake2s16::digest(data).as_slice() == self.pieces[piece].hash
        } else {
            false
        }
    }

    pub fn write_chunk(&self, piece_index: usize, chunk_index: usize, data: &[u8]) -> bool {
        if let Some(piece) = self.pieces.get(piece_index as usize) {
            let mut piece_state = piece.state.borrow_mut();
            if let PieceState::Incomplete(ref mut total, ref mut acquired) = *piece_state {
                if chunk_index > *total {
                    return false;
                };

                if !acquired.contains(&chunk_index) {
                    println!("piece {piece_index}: writing chunk {chunk_index} out of {total}");

                    unsafe {
                        (*piece.pointer.borrow_mut() as *mut u8)
                            .add(chunk_index * CHUNK_SIZE)
                            .copy_from(data.as_ptr(), data.len());
                    }

                    acquired.push(chunk_index);
                }

                if *total == acquired.len() {
                    self.inner.flush().unwrap();

                    let digest = Blake2s16::digest(unsafe {
                        std::slice::from_raw_parts(*piece.pointer.borrow(), piece.size)
                    })
                    .as_slice()
                        == piece.hash;

                    if digest {
                        println!("piece {piece_index}: verify passed!");
                        *piece_state = PieceState::Downloaded;
                    } else {
                        println!("piece {piece_index}: verify failed D:");
                        // if piece doesn't pass verification, clear it
                        acquired.clear();
                    }
                }

                return true;
            }
        }

        false
    }
    
    pub fn needed_pieces(&self) -> Vec<usize> {
        let mut pieces = Vec::new();
        for (i, piece) in self.pieces.iter().enumerate() {
            if *piece.state.borrow() != PieceState::Downloaded {
                pieces.push(i);
            }
        }

        pieces
    }
}

// the torrent equivalent
#[derive(Debug)]
pub struct CardboardBox {
    pub hash: [u8; 24],
    pub metadata: CardboardMetadata,
    pub base_path: PathBuf,
    pub files: Vec<MappedFile>,
}

impl CardboardBox {
    pub fn create(name: String, dir: impl AsRef<Path>) -> io::Result<CardboardBox> {
        let mut entries = WalkDir::new(dir.as_ref())
            .contents_first(true)
            .into_iter()
            .filter_entry(|e| !e.file_type().is_dir())
            .map(|e| e.map(|p| p.into_path()).map_err(|err| err.into()))
            .collect::<Result<Vec<PathBuf>, io::Error>>()?;
        entries.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));

        let mut files: Vec<MappedFile> = Vec::with_capacity(entries.len());
        let mut file_metadata: Vec<FileMetadata> = Vec::with_capacity(entries.len());

        let mut hasher = Blake2s24::new();

        for entry in entries {
            let f = OpenOptions::new()
                .write(true)
                .read(true)
                .create(true)
                .truncate(false)
                .open(&entry)?;

            let (f_hash, mapped_file) = MappedFile::from_whole_file(&f)?;
            hasher.update(f_hash);

            file_metadata.push(FileMetadata {
                path: entry,
                size: mapped_file.size,
                pieces: mapped_file
                    .pieces
                    .iter()
                    .map(|v| v.hash)
                    .collect::<Vec<PieceHash>>(),
            });

            files.push(mapped_file);
        }

        let metadata = CardboardMetadata {
            name,
            files: file_metadata,
        };

        hasher.update(&rmp_serde::to_vec(&metadata).unwrap());

        Ok(CardboardBox {
            hash: hasher.finalize().try_into().unwrap(),
            metadata,
            files,
            base_path: dir.as_ref().to_owned(),
        })
    }

    pub fn from_metadata(
        dir: impl AsRef<Path>,
        hash: [u8; 24],
        metadata: CardboardMetadata,
    ) -> io::Result<CardboardBox> {
        let mut files: Vec<MappedFile> = Vec::with_capacity(metadata.files.len());

        fs::create_dir_all(dir.as_ref())?;

        for entry in &metadata.files {
            let fpath = dir.as_ref().join(&entry.path);
            
            if let Some(p) = fpath.parent() {
                fs::create_dir_all(p)?;
            }

            if fpath.is_file() {
                let f = OpenOptions::new()
                    .write(true)
                    .read(true)
                    .create(false)
                    .truncate(false)
                    .open(&fpath)?;
                f.set_len(entry.size as u64)?;

                files.push(MappedFile::from_file_verified(&f, &entry.pieces)?);
            } else {
                let f = OpenOptions::new()
                    .write(true)
                    .read(true)
                    .create(true)
                    .truncate(true)
                    .open(&fpath)?;
                f.set_len(entry.size as u64)?;

                files.push(MappedFile::from_file_empty(&f, &entry.pieces)?);
            }
        }

        Ok(CardboardBox {
            hash: hash,
            base_path: dir.as_ref().to_owned(),
            metadata: metadata,
            files,
        })
    }

    pub fn needed_pieces(&self) -> Vec<(usize, Vec<usize>)> {
        let mut v = Vec::new();
        for (i, file) in self.files.iter().enumerate() {
            let pieces = file.needed_pieces();
            if !pieces.is_empty() {
                v.push((i, pieces));
            }
        }

        v
    }
}

// #[derive(Default)]
// pub struct State {
//     pub files: HashMap<[u8; 24], MappedFile>,
//     pub peers: HashSet<SocketAddr>,
// }

// impl State {
//     pub fn add_whole_file(&mut self, path: impl AsRef<Path>) -> io::Result<[u8; 24]> {
//         let name = path
//             .as_ref()
//             .file_name()
//             .unwrap()
//             .to_str()
//             .unwrap()
//             .to_owned();
//         let f = OpenOptions::new()
//             .write(true)
//             .read(true)
//             .truncate(false)
//             .open(path)?;

//         let (hash, mfile) = MappedFile::from_whole_file(&f, name)?;

//         self.files.insert(hash, mfile);

//         Ok(hash)
//     }

// }
