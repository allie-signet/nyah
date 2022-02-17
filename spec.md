# message pairs
SEARCHING FOR PEERS [UDP Broadcast] - broadcast at start.
IM HERE - response to a searching for peers message; indicates a node wants to peer with the searching node.

FIND METADATA [Key] - node is looking for metadata for the file specified by key
GOT METADATA [Key] - sends metadata to node that is searching for it

FIND PIECE [Key, Piece] - node is looking to download a piece of the file specified by key
GOT PIECE [Key, Piece] - node has piece of file specified by key

START DOWNLOAD [Key, Piece] - node wants to start download of the specified piece
UPLOAD [Key, Piece] - sends specified piece to node