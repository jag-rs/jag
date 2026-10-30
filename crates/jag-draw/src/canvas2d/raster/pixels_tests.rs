use super::Raster;
#[test]
fn dirty_pixel_writes_match_the_coordinate_definition() {
    let source: Vec<u8> = (0..3 * 2).flat_map(|i| [i + 1, 0, 0, 255]).collect();
    for dx in -4..5 {
        for dy in -3..4 {
            for x in -2..4 {
                for y in -2..3 {
                    for dw in [-4, 0, 2, 5] {
                        for dh in [-3, 0, 2, 4] {
                            let mut raster = Raster::new(4, 3, true).unwrap();
                            raster.put_pixels(&source, [3, 2], [dx, dy], [x, y, dw, dh]);
                            let (left, right) = (x.min(x + dw), x.max(x + dw));
                            let (top, bottom) = (y.min(y + dh), y.max(y + dh));
                            let expected: Vec<u8> = (0..3)
                                .flat_map(|py| {
                                    (0..4).flat_map(move |px| {
                                        let (sx, sy) = (px - dx, py - dy);
                                        if (0..3).contains(&sx)
                                            && (0..2).contains(&sy)
                                            && (left..right).contains(&sx)
                                            && (top..bottom).contains(&sy)
                                        {
                                            [(sy * 3 + sx + 1) as u8, 0, 0, 255]
                                        } else {
                                            [0, 0, 0, 0]
                                        }
                                    })
                                })
                                .collect();
                            assert_eq!(
                                raster.pixels(),
                                expected,
                                "dest={dx},{dy}; dirty={x},{y},{dw},{dh}"
                            );
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn malformed_or_extreme_regions_do_not_overflow_or_mutate_pixels() {
    let mut raster = Raster::new(2, 2, true).unwrap();
    assert!(!raster.put_pixels(&[1, 2, 3], [1, 1], [0, 0], [0, 0, 1, 1]));
    for n in [i64::MIN, i64::MAX] {
        assert!(!raster.put_pixels(&[1, 2, 3, 255], [1, 1], [n, n], [n, n, n, n]));
    }
    assert_eq!(raster.pixels(), [0; 16]);
}
