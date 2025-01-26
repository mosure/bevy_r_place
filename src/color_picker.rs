use bevy::prelude::*;
use bevy_inspector_egui::prelude::*;
use bevy_inspector_egui::quick::ResourceInspectorPlugin;


#[derive(Reflect, Resource, InspectorOptions)]
#[reflect(Resource, InspectorOptions)]
pub struct ColorPicker {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub hex: String,

    #[reflect(ignore)]
    previous_hex: String,
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
            hex: "#ffffff".to_string(),
            previous_hex: "#ffffff".to_string(),
        }
    }
}

impl ColorPicker {
    pub fn color(&self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }

    pub fn is_hex_color_valid(&self) -> bool {
        if self.hex.len() != 7 || !self.hex.starts_with('#') {
            return false;
        }
        self.hex.chars().skip(1).all(|c| c.is_ascii_hexdigit())
    }
}

pub fn update_color_picker(
    mut color_picker: ResMut<ColorPicker>,
) {
    if !color_picker.is_changed() {
        return;
    }

    let hex_changed = color_picker.hex != color_picker.previous_hex;
    if hex_changed {
        color_picker.previous_hex = color_picker.hex.clone();

        if color_picker.is_hex_color_valid() {
            let r = u8::from_str_radix(&color_picker.hex[1..3], 16).unwrap();
            let g = u8::from_str_radix(&color_picker.hex[3..5], 16).unwrap();
            let b = u8::from_str_radix(&color_picker.hex[5..7], 16).unwrap();

            color_picker.r = r;
            color_picker.g = g;
            color_picker.b = b;
        }

        return;
    }

    color_picker.hex = format!("#{:02x}{:02x}{:02x}", color_picker.r, color_picker.g, color_picker.b);
    color_picker.previous_hex = color_picker.hex.clone();
}


#[derive(Default)]
pub struct ColorPickerPlugin;
impl Plugin for ColorPickerPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ColorPicker>()
            .register_type::<ColorPicker>()
            .add_plugins(ResourceInspectorPlugin::<ColorPicker>::default())
            .add_systems(Update, (
                update_color_picker,
            ));
    }
}


