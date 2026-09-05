//! Clipboard I/O: reading the current contents, writing items back, and
//! watching for changes.

pub mod reader;
pub mod watcher;
pub mod writer;

pub use reader::read_snapshot;
pub use watcher::{Watcher, WatcherHandle};
pub use writer::{write_files, write_image, write_text};
