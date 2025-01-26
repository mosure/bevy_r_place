use bevy::prelude::*;


#[derive(Default)]
pub struct HeadlessPlugin;
impl Plugin for HeadlessPlugin {
    fn build(&self, _app: &mut App) {
        // TODO: setup app without windowing
    }
}
