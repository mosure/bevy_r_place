use bevy::{
    prelude::*,
    winit::WinitWindows,
};

pub struct WindowIconPlugin {
    icon_path: String,
}

impl WindowIconPlugin {
    pub fn new(icon_path: &str) -> Self {
        Self { icon_path: icon_path.to_string() }
    }
}

impl Plugin for WindowIconPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(WindowIcon {
                asset_path: self.icon_path.clone(),
                handle: Handle::default(),
            })
            .add_systems(PreStartup, load_icon_image)
            .add_systems(Update, set_window_icon);
    }
}

#[derive(Resource)]
struct WindowIcon {
    asset_path: String,
    handle: Handle<Image>,
}

fn load_icon_image(
    mut icon: ResMut<WindowIcon>,
    asset_server: Res<AssetServer>,
) {
    icon.handle = asset_server.load(&icon.asset_path);
}

fn set_window_icon(
    windows: NonSend<WinitWindows>,
    icon: Res<WindowIcon>,
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    mut complete: Local<bool>,
) {
    if *complete {
        return;
    }

    if let Some(load_state) = asset_server.get_load_state(&icon.handle) {
        if !load_state.is_loaded() {
            return;
        }
    }

    let image = images
        .get(&icon.handle)
        .expect("image asset not found");

    let icon = winit::window::Icon::from_rgba(image.data.clone(), image.width(), image.height()).unwrap();
    for window in windows.windows.values() {
        window.set_window_icon(Some(icon.clone()));
    }

    *complete = true;
}
