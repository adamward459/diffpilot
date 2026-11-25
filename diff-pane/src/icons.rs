use gpui::{Hsla, IntoElement, PathBuilder, Pixels, Point, Styled, Window, canvas, point, px};

type Shape = (&'static [(f32, f32)], bool);

fn paint_shape(window: &mut Window, origin: Point<Pixels>, scale: f32, width: f32, color: Hsla, shape: &Shape) {
    let points: Vec<Point<Pixels>> = shape
        .0
        .iter()
        .map(|&(x, y)| point(origin.x + px(x * scale), origin.y + px(y * scale)))
        .collect();
    let mut builder = PathBuilder::stroke(px(width));
    builder.add_polygon(&points, shape.1);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

fn icon(size: f32, color: Hsla, width: f32, shapes: &'static [Shape]) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            for shape in shapes {
                paint_shape(window, bounds.origin, size / 16.0, width, color, shape);
            }
        },
    )
    .w(px(size))
    .h(px(size))
    .flex_none()
}

pub fn chevron(down: bool, size: f32, color: Hsla) -> impl IntoElement {
    const RIGHT: &[Shape] = &[(&[(5.5, 3.0), (10.5, 8.0), (5.5, 13.0)], false)];
    const DOWN: &[Shape] = &[(&[(3.0, 5.5), (8.0, 10.5), (13.0, 5.5)], false)];
    icon(size, color, 1.6, if down { DOWN } else { RIGHT })
}

pub fn check(size: f32, color: Hsla) -> impl IntoElement {
    const SHAPES: &[Shape] = &[(&[(3.2, 8.6), (6.6, 12.0), (12.8, 4.4)], false)];
    icon(size, color, 2.0, SHAPES)
}

pub fn document(size: f32, color: Hsla) -> impl IntoElement {
    const SHAPES: &[Shape] = &[
        (&[(3.5, 1.5), (9.5, 1.5), (12.5, 4.5), (12.5, 14.5), (3.5, 14.5)], true),
        (&[(9.5, 1.5), (9.5, 4.5), (12.5, 4.5)], false),
    ];
    icon(size, color, 1.1, SHAPES)
}

pub fn bubble(size: f32, color: Hsla) -> impl IntoElement {
    const SHAPES: &[Shape] = &[(
        &[
            (2.0, 2.5),
            (14.0, 2.5),
            (14.0, 11.0),
            (7.0, 11.0),
            (4.0, 14.0),
            (4.0, 11.0),
            (2.0, 11.0),
        ],
        true,
    )];
    icon(size, color, 1.2, SHAPES)
}

pub fn plus(size: f32, color: Hsla) -> impl IntoElement {
    const SHAPES: &[Shape] = &[(&[(8.0, 3.5), (8.0, 12.5)], false), (&[(3.5, 8.0), (12.5, 8.0)], false)];
    icon(size, color, 1.6, SHAPES)
}
