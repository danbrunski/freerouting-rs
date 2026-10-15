use freerouting_geometry::FloatPoint;

/// The transformation between DSN file coordinates and board coordinates.
///
/// Port of `io.CoordinateTransform`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinateTransform {
    scale_factor: f64,
    base_x: f64,
    base_y: f64,
}

impl CoordinateTransform {
    pub fn new(scale_factor: f64, base_x: f64, base_y: f64) -> Self {
        CoordinateTransform {
            scale_factor,
            base_x,
            base_y,
        }
    }

    /// Board units per DSN unit.
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub fn board_to_dsn(&self, value: f64) -> f64 {
        value / self.scale_factor
    }

    pub fn board_to_dsn_point(&self, point: FloatPoint) -> [f64; 2] {
        [
            self.board_to_dsn(point.x) + self.base_x,
            self.board_to_dsn(point.y) + self.base_y,
        ]
    }

    pub fn board_to_dsn_rel(&self, point: FloatPoint) -> [f64; 2] {
        [self.board_to_dsn(point.x), self.board_to_dsn(point.y)]
    }

    pub fn dsn_to_board(&self, value: f64) -> f64 {
        value * self.scale_factor
    }

    pub fn dsn_to_board_point(&self, x: f64, y: f64) -> FloatPoint {
        FloatPoint::new(
            self.dsn_to_board(x - self.base_x),
            self.dsn_to_board(y - self.base_y),
        )
    }

    pub fn dsn_to_board_rel(&self, x: f64, y: f64) -> FloatPoint {
        FloatPoint::new(self.dsn_to_board(x), self.dsn_to_board(y))
    }
}
