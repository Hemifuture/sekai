#[derive(Debug, Clone)]
pub struct CellsData {
    pub height: Vec<u8>,
}

impl CellsData {
    pub fn new(cells_count: usize) -> Self {
        Self {
            height: vec![0; cells_count],
        }
    }
}
