//! Starter: a Bevy app on Android with a working soft keyboard.
//!
//! An ordinary Bevy `TextInput` field; `OxydroidPlugin` connects it to the Android keyboard.
//! Tap the field to type: accents, any script, emoji, suggestions, swipe and voice all work.

use bevy::{
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    render::{
        RenderPlugin,
        settings::{InstanceFlags, WgpuSettings},
    },
    text::{EditableText, FontSource, TextCursorStyle},
    ui_widgets::TextInput,
};
use oxydroid_bevy::{AndroidPlatform, OxydroidPlugin};

mod selftest; // optional: `--es oxytest 1` on launch runs a keyboard check. Safe to delete.

#[bevy_main]
fn main() {
    // wgpu's debug labels (on in debug builds) crash the Android emulator's Vulkan driver
    // (vk_common_SetDebugUtilsObjectNameEXT). Real phones don't need them either.
    let mut wgpu = WgpuSettings::default();
    wgpu.instance_flags.remove(InstanceFlags::DEBUG);
    let render = RenderPlugin {
        render_creation: wgpu.into(),
        ..default()
    };

    App::new()
        .add_plugins((DefaultPlugins.set(render), OxydroidPlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, (echo_value, pad_for_system_bars))
        .add_plugins(selftest::plugin)
        .run();
}

#[derive(Component)]
struct Root;

/// The text field.
#[derive(Component)]
pub struct Field;

#[derive(Component)]
struct Echo;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands
        .spawn((
            Root,
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(12),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((Text::new("Tap the field and type:"), font(26.0)));
            p.spawn((
                Field,
                Node {
                    width: percent(100),
                    padding: UiRect::all(px(10)),
                    ..default()
                },
                TextInput,
                EditableText::new(""),
                TabIndex(0),                // makes it focusable (tap to focus)
                TextCursorStyle::default(), // shows the caret while focused
                font(30.0),
                BackgroundColor(Color::srgb(0.12, 0.12, 0.16)),
            ));
            p.spawn((Echo, Text::new(""), font(22.0)));
        });
}

/// Show what the field holds, to see the keyboard's text arrive.
fn echo_value(
    fields: Query<&EditableText, (With<Field>, Changed<EditableText>)>,
    mut echo: Query<&mut Text, With<Echo>>,
) {
    if let (Ok(field), Ok(mut text)) = (fields.single(), echo.single_mut()) {
        text.0 = format!("value: {}", field.value());
    }
}

/// Keep the UI below the status bar: Android 16 draws apps edge-to-edge.
fn pad_for_system_bars(
    platform: Res<AndroidPlatform>,
    windows: Query<&Window>,
    mut root: Query<&mut Node, With<Root>>,
) {
    if !platform.is_changed() {
        return;
    }
    let (Ok(window), Ok(mut node)) = (windows.single(), root.single_mut()) else {
        return;
    };
    node.padding.top = px(platform.insets.top as f32 / window.scale_factor() + 16.0);
}

/// The system sans-serif font (Roboto on most phones) with the system fallbacks, so accents,
/// Arabic, CJK and emoji render. Needs Bevy's `system_font_discovery` feature.
fn font(size: f32) -> TextFont {
    TextFont {
        font: FontSource::Families("sans-serif".into()),
        ..TextFont::from_font_size(size)
    }
}
