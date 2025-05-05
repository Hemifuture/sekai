use super::cells_data::CellsData;

pub trait MapFeature {
    /// 获取特征ID
    fn id(&self) -> u16;

    /// 获取特征名称
    fn name(&self) -> &str;

    /// 获取特征类型
    fn feature_type(&self) -> FeatureType;

    /// 获取特征单元格列表（通过计算）
    fn cells<'a>(&self, cells_data: &'a CellsData) -> Vec<usize>;

    /// 判断单元格是否属于该特征
    fn contains_cell(&self, cell_id: usize, cells_data: &CellsData) -> bool;

    /// 添加单元格到该特征
    fn add_cell(&self, cell_id: usize, cells_data: &mut CellsData);

    /// 从该特征中移除单元格
    fn remove_cell(&self, cell_id: usize, cells_data: &mut CellsData);
}

pub enum FeatureType {
    State,
    Culture,
    Religion,
    Province,
    Biome,
}
