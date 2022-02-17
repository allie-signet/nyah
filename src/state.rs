pub use crate::messages::*;
pub use crate::*;

use memmap2::MmapMut;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::Path;

pub struct MappedFile {
    pub inner: MmapMut,
    pub name: String,
    pub size: u32,
    pub downloaded: bool,
    pub pieces: Vec<Piece>,
}

pub struct Piece {
    pub hash: [u8; 16],
    pub size: usize,
    pub downloaded: bool,
    pub pointer: RefCell<*const u8>,
}

impl MappedFile {
    pub fn from_whole_file(f: &File, name: String) -> io::Result<(FileHash, MappedFile)> {
        let mut file_hasher = Blake2s24::new();
        let mut piece_hasher = Blake2s16::new();
        let mmap = unsafe { MmapMut::map_mut(f)? };
        let mut piece_pointers: Vec<Piece> = Vec::new();

        for piece in mmap.chunks(CHUNK_SIZE) {
            file_hasher.update(&piece);
            piece_hasher.update(&piece);

            piece_pointers.push(Piece {
                hash: piece_hasher.finalize_reset().into(),
                downloaded: true,
                size: piece.len(),
                pointer: RefCell::new(piece.as_ptr()),
            });
        }

        Ok((
            file_hasher.finalize().into(),
            MappedFile {
                inner: mmap,
                name,
                size: f.metadata()?.len() as u32,
                downloaded: true,
                pieces: piece_pointers,
            },
        ))
    }

    pub fn from_file_verified(
        f: &File,
        piece_hashes: &[[u8; 16]],
        name: String,
    ) -> io::Result<MappedFile> {
        let mmap = unsafe { MmapMut::map_mut(f)? };
        let piece_iter = mmap.chunks(CHUNK_SIZE);

        if piece_hashes.len() != piece_iter.len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let mut piece_hasher = Blake2s16::new();
        let mut piece_pointers: Vec<Piece> = Vec::new();

        for (i, piece) in piece_iter.enumerate() {
            piece_hasher.update(&piece);

            let hash = piece_hasher.finalize_reset().into();

            piece_pointers.push(Piece {
                hash,
                downloaded: piece_hashes[i] == hash,
                size: piece.len(),
                pointer: RefCell::new(piece.as_ptr()),
            });
        }

        Ok(MappedFile {
            inner: mmap,
            name,
            size: f.metadata()?.len() as u32,
            downloaded: piece_pointers.iter().all(|v| v.downloaded),
            pieces: piece_pointers,
        })
    }

    pub fn from_file_empty(
        f: &File,
        piece_hashes: &[[u8; 16]],
        name: String,
    ) -> io::Result<MappedFile> {
        let mmap = unsafe { MmapMut::map_mut(f)? };
        let piece_iter = mmap.chunks(CHUNK_SIZE);

        if piece_hashes.len() != piece_iter.len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let mut piece_pointers: Vec<Piece> = Vec::new();

        for (i, piece) in piece_iter.enumerate() {
            piece_pointers.push(Piece {
                hash: piece_hashes[i],
                downloaded: false,
                size: piece.len(),
                pointer: RefCell::new(piece.as_ptr()),
            });
        }

        Ok(MappedFile {
            inner: mmap,
            name,
            size: f.metadata()?.len() as u32,
            downloaded: false,
            pieces: piece_pointers,
        })
    }
}

#[derive(Default)]
pub struct State {
    pub files: HashMap<[u8; 24], MappedFile>,
    pub peers: HashSet<SocketAddr>,
}

impl State {
    pub fn add_whole_file(&mut self, path: impl AsRef<Path>) -> io::Result<[u8; 24]> {
        let name = path
            .as_ref()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let f = OpenOptions::new()
            .write(true)
            .read(true)
            .truncate(false)
            .open(path)?;

        let (hash, mfile) = MappedFile::from_whole_file(&f, name)?;

        self.files.insert(hash, mfile);

        Ok(hash)
    }

    pub fn create_or_add_file(
        &mut self,
        path: impl AsRef<Path>,
        hash: [u8; 24],
        metadata: FileMetadata,
    ) -> io::Result<()> {
        let path = path.as_ref();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if path.is_file() {
            let f = OpenOptions::new()
                .write(true)
                .read(true)
                .truncate(false)
                .open(path)?;

            f.set_len(metadata.size as u64)?;
            self.files.insert(
                hash,
                MappedFile::from_file_verified(&f, &metadata.pieces, name)?,
            );
        } else {
            let f = OpenOptions::new()
                .write(true)
                .read(true)
                .create(true)
                .open(path)?;

            f.set_len(metadata.size as u64)?;
            self.files.insert(
                hash,
                MappedFile::from_file_empty(&f, &metadata.pieces, name)?,
            );
        }

        Ok(())
    }

    pub fn get_metadata(&self, key: &[u8; 24]) -> Option<Vec<u8>> {
        if let Some(f) = self.files.get(key) {
            Some(
                FileMetadata {
                    name: f.name.clone(),
                    size: f.size,
                    pieces: f.pieces.iter().map(|v| v.hash).collect(),
                }
                .to_bytes(),
            )
        } else {
            None
        }
    }

    pub fn has_piece(&self, key: &[u8; 24], piece: u32) -> bool {
        self.files
            .get(key)
            .and_then(|f| f.pieces.get(piece as usize).map(|v| v.downloaded))
            .unwrap_or(false)
    }

    pub fn read_piece(&self, key: &[u8; 24], piece: u32) -> Option<&[u8]> {
        if let Some(piece) = self.files.get(key).and_then(|f| {
            f.pieces
                .get(piece as usize)
                .and_then(|v| if v.downloaded { Some(v) } else { None })
        }) {
            if let Ok(ptr) = piece.pointer.try_borrow() {
                return Some(unsafe { std::slice::from_raw_parts(*ptr, piece.size) });
            }
        }

        None
    }

    pub fn write_piece(&self, key: &[u8; 24], piece: u32, data: &[u8]) -> bool {
        if let Some(piece) = self.files.get(key).and_then(|f| {
            f.pieces
                .get(piece as usize)
                .and_then(|v| if !v.downloaded { Some(v) } else { None })
        }) {
            if Blake2s16::digest(data).as_slice() != piece.hash {
                return false;
            }

            if let Ok(ptr) = piece.pointer.try_borrow_mut() {
                unsafe {
                    (*ptr as *mut u8).copy_from(data.as_ptr(), piece.size);
                }

                return true;
            }
        }

        false
    }

    pub fn verify_piece(&mut self, key: &[u8; 24], piece_idx: u32) -> bool {
        if self.files.contains_key(key) {
            self.files[key].inner.flush().unwrap();
        }

        if let Some(piece) = self
            .files
            .get_mut(key)
            .and_then(|f| f.pieces.get_mut(piece_idx as usize))
        {
            let ptr = piece.pointer.borrow();
            let slice = unsafe { std::slice::from_raw_parts(*ptr, piece.size) };

            if Blake2s16::digest(slice).as_slice() == piece.hash {
                piece.downloaded = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn write_partial_piece(
        &self,
        key: &[u8; 24],
        piece: u32,
        offset: u32,
        data: &[u8],
    ) -> bool {
        if let Some(piece) = self.files.get(key).and_then(|f| {
            f.pieces
                .get(piece as usize)
                .and_then(|v| if !v.downloaded { Some(v) } else { None })
        }) {
            if let Ok(ptr) = piece.pointer.try_borrow_mut() {
                unsafe {
                    (*ptr as *mut u8)
                        .add(offset as usize)
                        .copy_from(data.as_ptr(), data.len());
                }

                return true;
            }
        }

        false
    }
}

#[derive(Debug)]
pub struct FileMetadata {
    pub name: String,
    pub size: u32,
    pub pieces: Vec<[u8; 16]>,
}

impl FileMetadata {
    pub fn from_bytes(mut bytes: &[u8]) -> io::Result<FileMetadata> {
        let mut u32_buf: [u8; 4] = [0; 4];

        bytes.read_exact(&mut u32_buf)?;
        let mut string_buf = vec![0; u32::from_le_bytes(u32_buf) as usize];

        bytes.read_exact(&mut string_buf)?;
        let name = String::from_utf8(string_buf).unwrap();

        bytes.read_exact(&mut u32_buf)?;
        let file_size = u32::from_le_bytes(u32_buf);

        let mut piece_hashes = Vec::with_capacity(bytes.len() / 16);
        let mut piece_buf: [u8; 16] = [0; 16];

        while !bytes.is_empty() {
            bytes.read_exact(&mut piece_buf)?;
            piece_hashes.push(std::mem::take(&mut piece_buf));
        }

        Ok(FileMetadata {
            name,
            size: file_size,
            pieces: piece_hashes,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + self.name.len() + self.pieces.len() * 16);
        bytes.extend((self.name.len() as u32).to_le_bytes());
        bytes.extend(self.name.as_bytes());
        bytes.extend(self.size.to_le_bytes());
        bytes.extend(self.pieces.iter().flatten());
        bytes
    }
}
