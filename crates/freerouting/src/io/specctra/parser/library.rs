//! The library scope: padstacks and images (packages).
//!
//! Port of `io.specctra.parser.Library.readScope`,
//! `Library.readPadstackScope` and `Package.readScope`.

use freerouting_dsn::ast;
use freerouting_geometry::{ConvexShape, IntPoint, Point, PolylineShape, Shape, TileShape};

use super::ReadContext;
use super::layer::DsnLayerStructure;
use super::shape::{DsnArea, DsnShape, transform_area_to_board_rel};
use crate::io::CoordinateTransform;
use crate::library::{PackageKeepout, PackagePin, Packages, Padstacks};
use crate::util::remove_dot_numbers;

fn simplify_piece(piece: TileShape) -> ConvexShape {
    match piece {
        TileShape::Simplex(s) => ConvexShape::Tile(s.simplify()),
        other => ConvexShape::Tile(other),
    }
}

/// The convex pieces of a padstack shape.
fn convex_pieces(
    ctx: &mut ReadContext,
    shape: Option<Shape>,
    padstack_name: &str,
) -> Vec<ConvexShape> {
    let Some(shape) = shape else {
        return Vec::new();
    };
    match shape {
        Shape::Circle(c) => vec![ConvexShape::Circle(c)],
        Shape::Polyline(PolylineShape::Tile(t)) => vec![ConvexShape::Tile(t)],
        Shape::Polyline(PolylineShape::Polygon(polygon)) => {
            if let Some(pieces) = polygon.split_to_convex()
                && !pieces.is_empty()
            {
                return pieces.into_iter().map(simplify_piece).collect();
            }
            let hull = Shape::from(polygon.convex_hull());
            let fallback = hull.split_to_convex();
            if fallback.as_ref().is_some_and(|f| f.len() != 1) {
                ctx.log(format!(
                    "Library.read_padstack_scope: convex shape expected at '{padstack_name}'"
                ));
            }
            fallback
                .and_then(|f| f.into_iter().next())
                .map(simplify_piece)
                .into_iter()
                .collect()
        }
    }
}

/// Java `Library.readPadstackScope` after the scope was read: builds the
/// per-layer convex shapes and adds the padstack.
fn add_padstack(
    ctx: &mut ReadContext,
    padstack: &ast::Padstack,
    structure: &DsnLayerStructure,
    transform: &CoordinateTransform,
    padstacks: &mut Padstacks,
) -> bool {
    let name = &padstack.name;
    ctx.scope_identifier = name.clone();
    if padstacks.get_by_name(name).is_some() {
        return true;
    }
    let shapes: Vec<DsnShape> = padstack
        .shapes
        .iter()
        .filter_map(|s| DsnShape::resolve(s, Some(structure)))
        .collect();
    if shapes.is_empty() {
        ctx.log(format!(
            "Library.read_padstack_scope: shape not found for padstack with name '{name}'"
        ));
        return true;
    }
    let layer_count = structure.layers.len();
    let mut is_generic_default = vec![false; layer_count];
    let mut layer_pieces: Vec<Vec<ConvexShape>> = vec![Vec::new(); layer_count];
    for pad_shape in &shapes {
        let pieces = convex_pieces(ctx, pad_shape.transform_to_board_rel(transform), name);
        let mut validated = Vec::with_capacity(pieces.len());
        for mut piece in pieces {
            if piece.to_shape().dimension() < 2 {
                ctx.log(format!(
                    "Library.read_padstack_scope: the shape of padstack '{name}' is not an area. \
                     We will enlarge it as a workaround, but it may result in unintended \
                     consequences."
                ));
                piece = piece.offset(1.0);
            }
            if piece.to_shape().dimension() >= 2 {
                validated.push(piece);
            }
        }
        if pad_shape.layer.is_pcb() || pad_shape.layer.is_signal_pseudo_layer() {
            for l in 0..layer_count {
                if !is_generic_default[l] && layer_pieces[l].is_empty() {
                    is_generic_default[l] = true;
                }
                layer_pieces[l].extend(validated.iter().cloned());
            }
        } else {
            let shape_layer = structure.get_no(&pad_shape.layer.name);
            if shape_layer < 0 || shape_layer as usize >= layer_count {
                ctx.log(format!(
                    "Library.read_padstack_scope: layer number found at '{name}'"
                ));
                return false;
            }
            let l = shape_layer as usize;
            if is_generic_default[l] {
                layer_pieces[l].clear();
                is_generic_default[l] = false;
            }
            layer_pieces[l].extend(validated);
        }
    }
    let mut padstack_shapes: Vec<Option<ConvexShape>> = vec![None; layer_count];
    let mut auxiliary_shapes: Vec<Option<Vec<ConvexShape>>> = vec![None; layer_count];
    let mut has_non_convex = false;
    for (l, pieces) in layer_pieces.into_iter().enumerate() {
        if pieces.is_empty() {
            continue;
        }
        if pieces.len() == 1 {
            padstack_shapes[l] = pieces.into_iter().next();
            continue;
        }
        has_non_convex = true;
        ctx.log(format!(
            "Library.read_padstack_scope: Padstack '{name}' contains non-convex custom geometry \
             ({} convex tiles on layer {l}). Decomposing into primary core pin and auxiliary \
             copper obstacles.",
            pieces.len()
        ));
        let origin = Point::Int(IntPoint::new(0, 0));
        let core_index = pieces
            .iter()
            .position(|p| p.to_shape().contains(&origin))
            .or_else(|| {
                let mut best = None;
                let mut max_area = -1.0;
                for (i, p) in pieces.iter().enumerate() {
                    let area = p.to_shape().area();
                    if area > max_area {
                        max_area = area;
                        best = Some(i);
                    }
                }
                best
            })
            .unwrap_or(0);
        let mut aux = Vec::with_capacity(pieces.len() - 1);
        for (i, piece) in pieces.into_iter().enumerate() {
            if i == core_index {
                padstack_shapes[l] = Some(piece);
            } else {
                aux.push(piece);
            }
        }
        auxiliary_shapes[l] = Some(aux);
    }
    let id = padstacks.add(
        name.clone(),
        padstack_shapes,
        auxiliary_shapes,
        padstack.attach.unwrap_or(true),
        padstack.absolute.unwrap_or(false),
    );
    if has_non_convex && let Some(p) = padstacks.get_mut(id) {
        p.has_non_convex_geometry = true;
    }
    true
}

/// Java `Library.generateMissingKeepoutNames`.
fn generate_missing_keepout_names(prefix: &str, keepouts: &mut [DsnArea]) {
    if keepouts.iter().all(|k| k.name.is_some()) {
        return;
    }
    for (i, keepout) in keepouts.iter_mut().enumerate() {
        keepout.name = Some(format!("{prefix}{}", i + 1));
    }
}

/// The padstack named `name`, also without `.<digits>` parts.
pub(crate) fn find_padstack<'a>(
    padstacks: &'a Padstacks,
    name: &str,
) -> Option<&'a crate::library::Padstack> {
    padstacks
        .get_by_name(name)
        .or_else(|| padstacks.get_by_name(&remove_dot_numbers(name)))
}

/// Reads the library scope. Returns false where Java's reader does.
pub(crate) fn read_library(ctx: &mut ReadContext, library: &ast::Library) -> bool {
    let structure = ctx.layer_structure.clone();
    let transform = ctx.coordinate_transform;
    let layer_count = ctx.board().layer_count();
    let mut padstacks = Padstacks::new(layer_count);
    for padstack in &library.padstacks {
        if !add_padstack(ctx, padstack, &structure, &transform, &mut padstacks) {
            ctx.board().library.padstacks = padstacks;
            return false;
        }
    }
    let mut packages = Packages::new();
    for image in &library.images {
        ctx.scope_identifier = image.name.clone();
        let mut pins = Vec::with_capacity(image.pins.len());
        for pin in &image.pins {
            let Some(padstack) = find_padstack(&padstacks, &pin.padstack) else {
                ctx.log(format!(
                    "Library.read_scope: board padstack '{}' not found at '{}'",
                    pin.padstack, image.name
                ));
                ctx.board().library.padstacks = padstacks;
                ctx.board().library.packages = packages;
                return false;
            };
            let relative = transform.dsn_to_board_rel(pin.at.x, pin.at.y);
            pins.push(PackagePin::new(
                pin.name.clone(),
                padstack.id,
                relative,
                pin.rotation.unwrap_or(0.0),
            ));
        }
        let outline_dsn: Vec<DsnShape> = image
            .outlines
            .iter()
            .filter_map(|s| DsnShape::resolve(s, Some(&structure)))
            .collect();
        let mut outlines = Vec::with_capacity(outline_dsn.len());
        let mut outline_widths = Vec::with_capacity(outline_dsn.len());
        let mut outline_is_closed = Vec::with_capacity(outline_dsn.len());
        for shape in &outline_dsn {
            outlines.push(shape.transform_to_board_rel(&transform));
            if shape.is_path() {
                outline_widths.push(shape.width);
                let c = &shape.coor;
                outline_is_closed
                    .push(c.len() >= 4 && c[0] == c[c.len() - 2] && c[1] == c[c.len() - 1]);
            } else {
                outline_widths.push(0.0);
                outline_is_closed.push(true);
            }
        }
        let mut keepout_lists: [Vec<DsnArea>; 3] = Default::default();
        for keepout in &image.keepouts {
            let index = match keepout.kind {
                ast::KeepoutKind::Keepout => 0,
                ast::KeepoutKind::ViaKeepout => 1,
                ast::KeepoutKind::PlaceKeepout => 2,
            };
            match DsnArea::resolve(&keepout.area, Some(&structure), false) {
                Some(area) => keepout_lists[index].push(area),
                None => ctx.log(format!(
                    "Package.read_scope: could not read keepout area of package '{}'",
                    image.name
                )),
            }
        }
        generate_missing_keepout_names("keepout_", &mut keepout_lists[0]);
        generate_missing_keepout_names("via_keepout_", &mut keepout_lists[1]);
        generate_missing_keepout_names("place_keepout_", &mut keepout_lists[2]);
        let [keepouts, via_keepouts, place_keepouts] = keepout_lists.map(|list| {
            list.into_iter()
                .filter_map(|k| {
                    let layer = k.layer().no;
                    // Java fails with an exception on an area it can't
                    // convert; it is left out here.
                    let area = transform_area_to_board_rel(&k.shapes, &transform)?;
                    Some(PackageKeepout {
                        name: k.name.unwrap_or_default(),
                        area,
                        layer,
                    })
                })
                .collect::<Vec<_>>()
        });
        packages.add(
            image.name.clone(),
            pins,
            Some(outlines),
            Some(outline_widths),
            Some(outline_is_closed),
            keepouts,
            via_keepouts,
            place_keepouts,
            image.side != Some(ast::Side::Back),
        );
    }
    let board = ctx.board();
    board.library.padstacks = padstacks;
    board.library.packages = packages;
    true
}
