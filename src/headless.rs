use bevy::{
    prelude::*,
    log::LogPlugin,
};


#[derive(Default)]
pub struct HeadlessPlugin;
impl Plugin for HeadlessPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(LogPlugin::default());
        app.add_plugins(MinimalPlugins);
    }
}
