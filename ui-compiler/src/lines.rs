//! Byte offsets to 1-based line numbers in the whole `.svelte` file.

pub struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i + 1));
        Lines { starts }
    }

    /// The line the byte at `offset` is on.
    pub fn line(&self, offset: usize) -> u32 {
        self.starts.partition_point(|&s| s <= offset) as u32
    }
}
