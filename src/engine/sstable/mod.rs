//! SSTable — 持久化有序 K-V 文件 (Phase4).

mod block;
mod block_io;
mod builder;
mod filename;
mod footer;
mod handle;
mod index;
mod iterator;
mod meta;
mod properties;
mod reader;

pub use block::{Block, BlockBuilder, BlockIterator};
pub use block_io::{read_block_bytes, read_block_cached, read_block_from_file, write_block};
pub use builder::SSTableBuilder;
pub use filename::{parse_sstable_filename, sstable_path};
pub use footer::{Footer, FOOTER_SIZE, MAGIC_NUMBER};
pub use handle::BlockHandle;
pub use index::{find_block_handle, IndexBlock, IndexBlockBuilder, IndexEntry};
pub use iterator::SSTableIterator;
pub use properties::SstProperties;
pub use reader::SSTableReader;

pub(crate) const BLOCK_TRAILER_SIZE: usize = 5;
