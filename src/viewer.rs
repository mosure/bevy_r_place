use bevy::{
    prelude::*,
    app::AppExit,
    color::palettes::tailwind::*,
    picking::pointer::PointerInteraction,
};
use bevy_pancam::{
    PanCam,
    PanCamPlugin,
};

use crate::{
    prelude::*,
    chunk_crdt::{
        ChunkedCanvas,
        Pixel,
    },
    color_picker::{
        ColorPicker,
        ColorPickerPlugin,
    },
    network::PixelUpdateMsg,
    time::epoch_time_seconds,
};


#[derive(Resource, Clone, Debug, Default)]
pub struct CanvasSink {
    pub image: Handle<Image>,
}

pub fn extract_chunk_crdt(
    canvas: Res<ChunkedCanvas>,
    sink: Res<CanvasSink>,
    mut images: ResMut<Assets<Image>>,
) {
    if !canvas.is_changed() {
        return;
    }

    if images.contains(&sink.image) {
        images.insert(&sink.image, canvas.to_image());
    }
}


pub fn local_input_system(
    mouse: Res<ButtonInput<MouseButton>>,
    pointers: Query<&PointerInteraction>,
    mut gizmos: Gizmos,
    net: Res<BevyPlaceNodeHandle>,
    color_picker: Res<ColorPicker>,
    mut canvas: ResMut<ChunkedCanvas>,
) {
    for (mut point, _normal) in pointers
        .iter()
        .filter_map(|interaction| interaction.get_nearest_hit())
        .filter_map(|(_entity, hit)| hit.position.zip(hit.normal))
    {
        point.y = -point.y;
        let xy: Vec2 = point.xy().floor();

        let rect_center = (xy + Vec2::splat(0.5)) * Vec2::new(1.0, -1.0);
        gizmos.rect_2d(
            rect_center,
            Vec2::splat(1.0),
            YELLOW_400,
        );

        if mouse.just_pressed(MouseButton::Left) {
            let x = xy.x as i32 + WORLD_WIDTH as i32 / 2;
            let y = xy.y as i32 + WORLD_HEIGHT as i32 / 2;

            let x = x as u32;
            let y = y as u32;

            let color = color_picker.color.to_srgba().to_u8_array_no_alpha();

            // TODO: peer_id tracking
            let owner = [0; 32];

            // TODO: convert to lamport clock
            let timestamp = epoch_time_seconds();

            let pixel = Pixel {
                r: color[0],
                g: color[1],
                b: color[2],
                timestamp,
                owner,
            };
            canvas.set_pixel(x, y, pixel);

            let msg = PixelUpdateMsg {
                x,
                y,
                r: color[0],
                g: color[1],
                b: color[2],
                timestamp,
                owner,
            };
            net.outbound_tx.try_send(msg).ok();
        }
    }
}


// TODO: make mobile view-only? or support a better placement method + touch screen pan/zoom
fn setup_ui(
    canvas: Res<ChunkedCanvas>,
    mut commands: Commands,
    mut sink: ResMut<CanvasSink>,
    mut images: ResMut<Assets<Image>>,
) {
    sink.image = images.add(canvas.to_image());
    commands.spawn(Sprite::from_image(sink.image.clone()));

    commands.spawn((
        Camera2d,
        Camera {
            hdr: true,
            ..default()
        },
        OrthographicProjection {
            scale: 0.125,
            ..OrthographicProjection::default_2d()
        },
        // TODO: consider disabling zoom (or restricting it to a number of chunks to prevent requesting all chunks at once)
        PanCam {
            grab_buttons: vec![MouseButton::Middle, MouseButton::Right],
            max_scale: 1.0,
            min_scale: 0.01,
            ..default()
        },
    ));
}

fn esc_close(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: EventWriter<AppExit>
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.send(AppExit::Success);
    }
}


#[derive(Default)]
pub struct ViewerPlugin;
impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_arch = "wasm32")]
        let primary_window = Some(Window {
            // fit_canvas_to_parent: true,
            canvas: Some("#bevy".to_string()),
            mode: bevy::window::WindowMode::Windowed,
            prevent_default_event_handling: true,
            title: "bevy_place".to_string(),
            present_mode: bevy::window::PresentMode::AutoVsync,
            ..default()
        });

        #[cfg(not(target_arch = "wasm32"))]
        let primary_window = Some(Window {
            mode: bevy::window::WindowMode::Windowed,
            prevent_default_event_handling: false,
            resolution: (1920.0, 1080.0).into(),
            title: "bevy_place".to_string(),
            present_mode: bevy::window::PresentMode::AutoVsync,
            ..default()
        });

        app
            .add_plugins(
                DefaultPlugins.set(
                    ImagePlugin::default_nearest(),
                )
                .set(WindowPlugin {
                    primary_window,
                    ..default()
                })
            )
            .add_plugins(ColorPickerPlugin)
            .add_plugins(PanCamPlugin)
            .insert_resource(CanvasSink::default())
            .add_systems(Startup, setup_ui)
            .add_systems(Update, (
                esc_close,
                extract_chunk_crdt,
                local_input_system,
            ));

        #[cfg(all(feature = "native", feature = "viewer"))]
        app.add_plugins(crate::window_icon::WindowIconPlugin::new("images/bevy_r_place.png"));
    }
}
