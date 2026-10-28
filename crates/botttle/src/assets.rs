//! Assets compiled into the binary.
//!
//! Everything the UI draws ships inside the executable, so `cargo run` from a
//! fresh clone looks the same as an installed build — there is no asset
//! directory to lose.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, ImageSource, Resource, SharedString};

use crate::settings::IconStyle;
use crate::theme::Appearance;

/// Full-resolution app icons, scaled only when drawn by the UI or macOS.
pub const LOGO: &str = "logo.png";
pub const LOGO_TWILIGHT: &str = "logo-twilight.png";

/// An image built into the binary, for `img`.
///
/// Passing a bare name like `"logo.png"` to `img` doesn't do this: gpui first
/// checks whether the string parses as a URI, `logo.png` does (as a host
/// name), and the image is then fetched over the network and never appears.
pub fn embedded(path: &'static str) -> ImageSource {
    ImageSource::Resource(Resource::Embedded(SharedString::from(path)))
}

pub fn logo_for(style: IconStyle, appearance: Appearance) -> &'static str {
    match style {
        IconStyle::Bright => LOGO,
        IconStyle::Default => match appearance {
            Appearance::Light => LOGO,
            Appearance::Dark => LOGO_TWILIGHT,
        },
    }
}

/// The show/hide sidebar control. A mask: it takes the colour it is drawn in.
pub const SIDEBAR_ICON: &str = "sidebar.svg";
/// The new-tab control, drawn the same way so the two sit as a matched pair.
pub const PLUS_ICON: &str = "plus.svg";
/// Closes a tab from the sidebar.
pub const CLOSE_ICON: &str = "close.svg";
/// Opens the settings screen.
pub const SETTINGS_ICON: &str = "settings.svg";

const LOGO_BYTES: &[u8] = include_bytes!("../../../assets/logo.png");
const LOGO_TWILIGHT_BYTES: &[u8] = include_bytes!("../../../assets/logo-twilight.png");
const SIDEBAR_ICON_BYTES: &[u8] = include_bytes!("../assets/sidebar.svg");
const PLUS_ICON_BYTES: &[u8] = include_bytes!("../assets/plus.svg");
const CLOSE_ICON_BYTES: &[u8] = include_bytes!("../assets/close.svg");
const SETTINGS_ICON_BYTES: &[u8] = include_bytes!("../assets/settings.svg");

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            LOGO => Ok(Some(Cow::Borrowed(LOGO_BYTES))),
            LOGO_TWILIGHT => Ok(Some(Cow::Borrowed(LOGO_TWILIGHT_BYTES))),
            SIDEBAR_ICON => Ok(Some(Cow::Borrowed(SIDEBAR_ICON_BYTES))),
            PLUS_ICON => Ok(Some(Cow::Borrowed(PLUS_ICON_BYTES))),
            CLOSE_ICON => Ok(Some(Cow::Borrowed(CLOSE_ICON_BYTES))),
            SETTINGS_ICON => Ok(Some(Cow::Borrowed(SETTINGS_ICON_BYTES))),
            _ => Ok(None),
        }
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![
            SharedString::from(LOGO),
            SharedString::from(LOGO_TWILIGHT),
            SharedString::from(SIDEBAR_ICON),
            SharedString::from(PLUS_ICON),
            SharedString::from(CLOSE_ICON),
            SharedString::from(SETTINGS_ICON),
        ])
    }
}

/// macOS uses the same choice for the Dock and app switcher as the titlebar.
///
/// An image handed to the Dock is drawn exactly as given, so a full-bleed
/// logo would show as a hard-edged square beside icons that follow Apple's
/// grid. The logo is clipped to that grid's rounded square first.
#[cfg(target_os = "macos")]
pub fn apply_app_icon(style: IconStyle, appearance: Appearance) {
    use objc2::encode::{Encode, Encoding};
    use objc2::{class, msg_send, runtime::AnyObject};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }
    unsafe impl Encode for Size {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }
    unsafe impl Encode for Rect {
        const ENCODING: Encoding = Encoding::Struct(
            "CGRect",
            &[
                Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]),
                Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]),
            ],
        );
    }

    // Apple's macOS icon grid: an 824-point rounded square, centred on a
    // 1024-point canvas, with corners about 22% of its side.
    const CANVAS: f64 = 1024.0;
    const SHAPE: f64 = 824.0;
    const CORNER: f64 = 185.4;
    const NS_COMPOSITING_OPERATION_SOURCE_OVER: usize = 2;

    let bytes = if logo_for(style, appearance) == LOGO {
        LOGO_BYTES
    } else {
        LOGO_TWILIGHT_BYTES
    };

    unsafe {
        let data: *mut AnyObject = msg_send![class!(NSData), dataWithBytes: bytes.as_ptr().cast::<std::ffi::c_void>(), length: bytes.len()];
        if data.is_null() {
            return;
        }
        let logo: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let logo: *mut AnyObject = msg_send![logo, initWithData: data];
        if logo.is_null() {
            return;
        }

        let inset = (CANVAS - SHAPE) / 2.0;
        let shape = Rect {
            x: inset,
            y: inset,
            width: SHAPE,
            height: SHAPE,
        };
        let whole_image = Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };

        let icon: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let icon: *mut AnyObject =
            msg_send![icon, initWithSize: Size { width: CANVAS, height: CANVAS }];
        let _: () = msg_send![icon, lockFocus];
        let clip: *mut AnyObject = msg_send![
            class!(NSBezierPath),
            bezierPathWithRoundedRect: shape,
            xRadius: CORNER,
            yRadius: CORNER
        ];
        let _: () = msg_send![clip, addClip];
        let _: () = msg_send![
            logo,
            drawInRect: shape,
            fromRect: whole_image,
            operation: NS_COMPOSITING_OPERATION_SOURCE_OVER,
            fraction: 1.0f64
        ];
        let _: () = msg_send![icon, unlockFocus];

        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: icon];
        let _: () = msg_send![icon, release];
        let _: () = msg_send![logo, release];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_app_icon(_: IconStyle, _: Appearance) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_is_a_png_and_is_reachable_by_name() {
        for path in [LOGO, LOGO_TWILIGHT] {
            let bytes = Assets.load(path).expect("no error").expect("embedded");
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
            let width = u32::from_be_bytes(bytes[16..20].try_into().expect("IHDR width"));
            let height = u32::from_be_bytes(bytes[20..24].try_into().expect("IHDR height"));
            assert_eq!((width, height), (1024, 1024));
        }
    }

    #[test]
    fn default_icon_follows_appearance_and_fixed_choices_do_not() {
        assert_eq!(logo_for(IconStyle::Default, Appearance::Light), LOGO);
        assert_eq!(
            logo_for(IconStyle::Default, Appearance::Dark),
            LOGO_TWILIGHT
        );
        assert_eq!(logo_for(IconStyle::Bright, Appearance::Dark), LOGO);
    }

    #[test]
    fn unknown_assets_are_absent_rather_than_an_error() {
        assert!(Assets.load("nope.png").expect("no error").is_none());
    }

    #[test]
    fn embedded_images_are_never_mistaken_for_urls() {
        for path in [LOGO, LOGO_TWILIGHT] {
            assert!(matches!(
                embedded(path),
                ImageSource::Resource(Resource::Embedded(_))
            ));
        }
    }
}
