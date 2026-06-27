#[cfg(test)]
mod side_channel_opacity_tests {
    use super::super::{Canvas, ImageFitMode, ScrimDraw};
    use jag_draw::{ColorLinPremul, Painter, Rect, RoundedRadii, RoundedRect, Viewport};

    fn test_canvas() -> Canvas {
        let viewport = Viewport {
            width: 320,
            height: 240,
        };
        Canvas {
            viewport,
            painter: Painter::begin_frame(viewport),
            clear_color: None,
            text_provider: None,
            glyph_draws: Vec::new(),
            svg_draws: Vec::new(),
            image_draws: Vec::new(),
            backdrop_blur_draws: Vec::new(),
            raw_image_draws: Vec::new(),
            dpi_scale: 1.0,
            clip_stack: vec![None],
            rounded_clip_stack: vec![None],
            overlay_draws: Vec::new(),
            scrim_draws: Vec::<ScrimDraw>::new(),
            opacity_stack: vec![1.0],
        }
    }

    #[test]
    fn svg_side_channel_captures_effective_parent_opacity() {
        let mut canvas = test_canvas();
        canvas.push_opacity(0.5);
        canvas.push_opacity(0.25);

        canvas.draw_svg("icon.svg", [10.0, 20.0], [16.0, 16.0], 7);

        assert_eq!(canvas.svg_draws.len(), 1);
        assert_eq!(canvas.svg_draws[0].5, 0.125);
    }

    #[test]
    fn image_side_channel_captures_zero_parent_opacity() {
        let mut canvas = test_canvas();
        canvas.push_opacity(0.0);

        canvas.draw_image(
            "image.png",
            [0.0, 0.0],
            [24.0, 24.0],
            ImageFitMode::Contain,
            3,
        );

        assert_eq!(canvas.image_draws.len(), 1);
        assert_eq!(canvas.image_draws[0].5, 0.0);
    }

    #[test]
    fn side_channel_opacity_clamps_each_pushed_layer() {
        let mut canvas = test_canvas();
        canvas.push_opacity(0.5);
        canvas.push_opacity(2.0);

        canvas.draw_svg("icon.svg", [10.0, 20.0], [16.0, 16.0], 7);

        assert_eq!(canvas.svg_draws.len(), 1);
        assert_eq!(canvas.svg_draws[0].5, 0.5);
    }

    #[test]
    fn pop_opacity_restores_side_channel_parent_opacity() {
        let mut canvas = test_canvas();
        canvas.push_opacity(0.5);
        canvas.push_opacity(0.25);
        canvas.pop_opacity();

        canvas.draw_svg_styled(
            "icon.svg",
            [10.0, 20.0],
            [16.0, 16.0],
            jag_draw::SvgStyle::new().with_stroke(ColorLinPremul {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            }),
            7,
        );

        assert_eq!(canvas.svg_draws.len(), 1);
        assert_eq!(canvas.svg_draws[0].5, 0.5);
    }

    #[test]
    fn clipping_does_not_drop_side_channel_opacity() {
        let mut canvas = test_canvas();
        canvas.push_clip_rect(Rect {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
        });
        canvas.push_opacity(0.75);

        canvas.draw_svg("icon.svg", [4.0, 4.0], [16.0, 16.0], 7);

        assert_eq!(canvas.svg_draws.len(), 1);
        assert_eq!(canvas.svg_draws[0].5, 0.75);
    }

    #[test]
    fn svg_side_channel_captures_rounded_clip() {
        let mut canvas = test_canvas();
        canvas.push_clip_rounded_rect(RoundedRect {
            rect: Rect {
                x: 8.0,
                y: 12.0,
                w: 40.0,
                h: 40.0,
            },
            radii: RoundedRadii {
                tl: 20.0,
                tr: 20.0,
                br: 20.0,
                bl: 20.0,
            },
        });

        canvas.draw_svg("avatar.svg", [8.0, 12.0], [40.0, 40.0], 7);

        let rounded_clip = canvas.svg_draws[0]
            .8
            .expect("SVG draw should carry the active rounded clip");
        assert_eq!(rounded_clip.rect.x, 8.0);
        assert_eq!(rounded_clip.rect.y, 12.0);
        assert_eq!(rounded_clip.rect.w, 40.0);
        assert_eq!(rounded_clip.rect.h, 40.0);
        assert_eq!(rounded_clip.radii, [20.0, 20.0, 20.0, 20.0]);
    }
}

#[cfg(test)]
mod fill_rect_rounded_clip_tests {
    use super::super::Canvas;
    use jag_draw::{
        BoxShadowSpec, Brush, ColorLinPremul, Command, Painter, Rect, RoundedRadii, RoundedRect,
        Transform2D, Viewport,
    };

    fn test_canvas() -> Canvas {
        let viewport = Viewport {
            width: 320,
            height: 240,
        };
        Canvas {
            viewport,
            painter: Painter::begin_frame(viewport),
            clear_color: None,
            text_provider: None,
            glyph_draws: Vec::new(),
            svg_draws: Vec::new(),
            image_draws: Vec::new(),
            backdrop_blur_draws: Vec::new(),
            raw_image_draws: Vec::new(),
            dpi_scale: 1.0,
            clip_stack: vec![None],
            rounded_clip_stack: vec![None],
            overlay_draws: Vec::new(),
            scrim_draws: Vec::new(),
            opacity_stack: vec![1.0],
        }
    }

    fn solid() -> Brush {
        Brush::Solid(ColorLinPremul {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    }

    // A solid `fill_rect` inside a rounded `overflow:hidden` clip must follow the
    // ancestor's corner radii: route through the path-fill clipper (emitting a
    // `FillPath` that carries the rounded clip), never a square `DrawRect`. This
    // is the wildlife-journal plate ground-band square-corner bug.
    #[test]
    fn solid_fill_rect_under_rounded_clip_emits_rounded_clipped_path() {
        let mut canvas = test_canvas();
        canvas.push_clip_rounded_rect(RoundedRect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 40.0,
                h: 40.0,
            },
            radii: RoundedRadii {
                tl: 16.0,
                tr: 16.0,
                br: 16.0,
                bl: 16.0,
            },
        });
        // A band spanning the full width at the bottom — its corners sit exactly
        // where the rounded clip must round them.
        canvas.fill_rect(0.0, 30.0, 40.0, 10.0, solid(), 5);

        let cmds = &canvas.display_list().commands;
        let clip = cmds
            .iter()
            .find_map(|c| match c {
                Command::FillPath { clip, .. } => Some(*clip),
                _ => None,
            })
            .expect("solid fill_rect under a rounded clip should emit a FillPath")
            .expect("the FillPath should carry the active rounded clip");
        assert_eq!(clip.radii, [16.0, 16.0, 16.0, 16.0]);
        assert!(
            !cmds.iter().any(|c| matches!(c, Command::DrawRect { .. })),
            "must not fall back to an unrounded DrawRect"
        );
    }

    // No rounded clip active → keep the cheap DrawRect path (no behavior change).
    #[test]
    fn solid_fill_rect_without_rounded_clip_uses_plain_rect() {
        let mut canvas = test_canvas();
        canvas.fill_rect(0.0, 0.0, 40.0, 10.0, solid(), 5);

        let cmds = &canvas.display_list().commands;
        assert!(cmds.iter().any(|c| matches!(c, Command::DrawRect { .. })));
        assert!(!cmds.iter().any(|c| matches!(c, Command::FillPath { .. })));
    }

    #[test]
    fn clipped_box_shadow_carries_local_clip_under_transform() {
        let mut canvas = test_canvas();
        canvas.push_transform(Transform2D::translate(0.0, -120.0));
        canvas.push_clip_rect(Rect {
            x: 0.0,
            y: 120.0,
            w: 180.0,
            h: 90.0,
        });

        canvas.box_shadow(
            RoundedRect {
                rect: Rect {
                    x: 16.0,
                    y: 130.0,
                    w: 120.0,
                    h: 60.0,
                },
                radii: RoundedRadii {
                    tl: 6.0,
                    tr: 6.0,
                    br: 6.0,
                    bl: 6.0,
                },
            },
            BoxShadowSpec {
                offset: [0.0, 4.0],
                spread: 0.0,
                blur_radius: 16.0,
                color: ColorLinPremul {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.24,
                },
            },
            5,
        );

        let clip = canvas
            .display_list()
            .commands
            .iter()
            .find_map(|command| match command {
                Command::BoxShadow { clip, .. } => *clip,
                _ => None,
            })
            .expect("box shadow should carry the active clip");
        assert_eq!(clip.rect.x, 0.0);
        assert_eq!(clip.rect.y, 120.0);
        assert_eq!(clip.rect.w, 180.0);
        assert_eq!(clip.rect.h, 90.0);
        assert_eq!(clip.radii, [0.0; 4]);
    }

    #[test]
    fn clipped_box_shadow_carries_rounded_clip_radii_under_scroll_transform() {
        let mut canvas = test_canvas();
        canvas.push_clip_rounded_rect(RoundedRect {
            rect: Rect {
                x: 0.0,
                y: 24.0,
                w: 180.0,
                h: 120.0,
            },
            radii: RoundedRadii {
                tl: 8.0,
                tr: 10.0,
                br: 12.0,
                bl: 14.0,
            },
        });
        canvas.push_transform(Transform2D::translate(0.0, -48.0));

        canvas.box_shadow(
            RoundedRect {
                rect: Rect {
                    x: 12.0,
                    y: 88.0,
                    w: 120.0,
                    h: 48.0,
                },
                radii: RoundedRadii {
                    tl: 6.0,
                    tr: 6.0,
                    br: 6.0,
                    bl: 6.0,
                },
            },
            BoxShadowSpec {
                offset: [0.0, 8.0],
                spread: 0.0,
                blur_radius: 24.0,
                color: ColorLinPremul {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.24,
                },
            },
            7,
        );

        let clip = canvas
            .display_list()
            .commands
            .iter()
            .find_map(|command| match command {
                Command::BoxShadow { clip, .. } => *clip,
                _ => None,
            })
            .expect("box shadow should carry the active rounded clip");
        assert_eq!(clip.rect.x, 0.0);
        assert_eq!(clip.rect.y, 72.0);
        assert_eq!(clip.rect.w, 180.0);
        assert_eq!(clip.rect.h, 120.0);
        assert_eq!(clip.radii, [8.0, 10.0, 12.0, 14.0]);
    }
}

#[cfg(test)]
mod gradient_text_mask_tests {
    use super::super::helpers::tint_glyph_mask_with_gradient;
    use jag_draw::{GlyphMask, MaskFormat, RasterizedGlyph, SubpixelMask};

    #[test]
    fn gradient_tint_uploads_subpixel_masks_as_color_masks() {
        let glyph = RasterizedGlyph {
            offset: [0.0, 0.0],
            mask: GlyphMask::Subpixel(SubpixelMask {
                width: 2,
                height: 1,
                format: MaskFormat::Rgba8,
                data: vec![255, 255, 255, 0, 128, 128, 128, 0],
            }),
        };

        let tinted = tint_glyph_mask_with_gradient(
            &glyph,
            0.0,
            1.0,
            2.0,
            &[(0.0, [0.0, 1.0, 0.0, 1.0]), (1.0, [0.0, 0.5, 0.0, 1.0])],
        );

        let GlyphMask::Color(mask) = tinted.mask else {
            panic!("gradient text should upload as a color mask");
        };
        assert_eq!(mask.data.len(), 8);
        assert_eq!(mask.data[0], 0);
        assert_eq!(mask.data[1], 255);
        assert_eq!(mask.data[2], 0);
        assert_eq!(mask.data[3], 255);
        assert!(mask.data[5] > mask.data[4]);
        assert!(mask.data[5] > mask.data[6]);
        assert_eq!(mask.data[7], 128);
    }
}
