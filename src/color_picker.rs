use bevy::prelude::*;
use bevy_inspector_egui::prelude::*;
use bevy_inspector_egui::quick::ResourceInspectorPlugin;


#[derive(Reflect, Resource, InspectorOptions)]
#[reflect(Resource, InspectorOptions)]
pub struct ColorPicker {
    pub color: Color,
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self {
            color: Color::srgb(1.0, 1.0, 1.0),
        }
    }
}

fn random_color() -> Color {
    Color::srgb(
        rand::random::<f32>(),
        rand::random::<f32>(),
        rand::random::<f32>(),
    )
}

#[derive(Default)]
pub struct ColorPickerPlugin;
impl Plugin for ColorPickerPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(ColorPicker {
                color: random_color(),
            })
            .register_type::<ColorPicker>()
            .add_plugins(ResourceInspectorPlugin::<ColorPicker>::default());
    }
}
