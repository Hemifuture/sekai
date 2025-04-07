pub trait MapLayer {
    fn get_name(&self) -> &str;
    fn get_id(&self) -> &str;
    fn get_description(&self) -> &str;
    fn get_image(&self) -> &str;

    fn is_visible(&self) -> bool;
    fn set_visible(&mut self, visible: bool);
}
