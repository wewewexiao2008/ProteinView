use std::time::Instant;

/// 3x3 rotation matrix stored as row-major array.
type Mat3 = [[f64; 3]; 3];

/// 3D camera for viewing protein structures.
///
/// Rotation is stored as an accumulated 3x3 matrix rather than Euler angles.
/// User-initiated rotations (rotate_x/y/z) are applied via **left-multiplication**
/// of an incremental rotation matrix, which means the rotation happens in the
/// camera's local (screen-space) coordinate system.  This ensures that pressing
/// j/k always rotates around the screen's horizontal axis regardless of the
/// current orientation.
#[derive(Debug, Clone)]
pub struct Camera {
    /// Accumulated rotation matrix (row-major, 3x3).
    mat: Mat3,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub auto_rotate: bool,
    last_tick: Instant,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            mat: identity(),
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            auto_rotate: false,
            last_tick: Instant::now(),
        }
    }
}

/// A projected 2D point with depth
#[derive(Debug, Clone, Copy)]
pub struct Projected {
    pub x: f64,
    pub y: f64,
    pub z: f64, // depth for z-buffering
}

/// Pre-computed rotation matrix for a single frame.
///
/// Instead of six trig values (sin/cos for three Euler angles), we store the
/// full 3x3 matrix and zoom/pan values.  This is exactly as cheap to use
/// per-vertex as the previous trig-based approach (9 multiply-adds vs 12),
/// and avoids the gimbal-lock / axis-dependency problems of Euler angles.
#[derive(Debug, Clone, Copy)]
pub struct ProjectionCache {
    /// Row-major 3x3 rotation matrix.
    mat: Mat3,
    zoom: f64,
    pan_x: f64,
    pan_y: f64,
}

impl ProjectionCache {
    /// Project a 3D point to 2D using the cached matrix.
    ///
    /// The math is identical to [`Camera::project()`] but uses the pre-baked
    /// matrix so no per-vertex computation is wasted.
    #[inline]
    pub fn project(&self, x: f64, y: f64, z: f64) -> Projected {
        let x3 = self.mat[0][0] * x + self.mat[0][1] * y + self.mat[0][2] * z;
        let y3 = self.mat[1][0] * x + self.mat[1][1] * y + self.mat[1][2] * z;
        let z2 = self.mat[2][0] * x + self.mat[2][1] * y + self.mat[2][2] * z;

        Projected {
            x: -x3 * self.zoom + self.pan_x,
            y: y3 * self.zoom + self.pan_y,
            z: z2,
        }
    }

    /// Apply the camera rotation to a direction vector (no zoom/pan).
    #[inline]
    pub fn rotate_normal(&self, nx: f64, ny: f64, nz: f64) -> [f64; 3] {
        let rx = self.mat[0][0] * nx + self.mat[0][1] * ny + self.mat[0][2] * nz;
        let ry = self.mat[1][0] * nx + self.mat[1][1] * ny + self.mat[1][2] * nz;
        let rz = self.mat[2][0] * nx + self.mat[2][1] * ny + self.mat[2][2] * nz;
        [rx, ry, rz]
    }
}

// ---- Helper functions for 3x3 matrix algebra ----

/// Identity matrix.
#[inline]
fn identity() -> Mat3 {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

/// Multiply two 3x3 matrices: C = A * B.
#[inline]
fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut c = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    c
}

/// Build a rotation matrix around the X axis.
#[inline]
fn rot_x_mat(angle: f64) -> Mat3 {
    let (s, c) = angle.sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

/// Build a rotation matrix around the Y axis.
#[inline]
fn rot_y_mat(angle: f64) -> Mat3 {
    let (s, c) = angle.sin_cos();
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

/// Build a rotation matrix around the Z axis.
#[inline]
fn rot_z_mat(angle: f64) -> Mat3 {
    let (s, c) = angle.sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

impl Camera {
    const ROT_STEP: f64 = 0.1;
    const ZOOM_STEP: f64 = 0.1;
    const PAN_STEP: f64 = 2.0;

    /// Rotate around the camera's local X axis (screen horizontal).
    ///
    /// Left-multiplying the incremental rotation matrix applies the rotation
    /// in the camera's local frame rather than world space.
    pub fn rotate_x(&mut self, dir: f64) {
        let inc = rot_x_mat(dir * Self::ROT_STEP);
        self.mat = mul(&inc, &self.mat);
    }
    pub fn rotate_y(&mut self, dir: f64) {
        let inc = rot_y_mat(-dir * Self::ROT_STEP);
        self.mat = mul(&inc, &self.mat);
    }
    pub fn rotate_z(&mut self, dir: f64) {
        let inc = rot_z_mat(-dir * Self::ROT_STEP);
        self.mat = mul(&inc, &self.mat);
    }
    pub fn zoom_in(&mut self) {
        self.zoom *= 1.0 + Self::ZOOM_STEP;
    }
    pub fn zoom_out(&mut self) {
        self.zoom *= 1.0 - Self::ZOOM_STEP;
    }
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.pan_x += dx * Self::PAN_STEP;
        self.pan_y += dy * Self::PAN_STEP;
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Auto-rotate speed in radians per second (~0.6 rad/s = one full turn in ~10s).
    const AUTO_ROTATE_SPEED: f64 = 0.6;

    /// Maximum dt (in seconds) that a single tick can apply.  This prevents
    /// the protein from "jumping" when a frame takes longer than expected or
    /// when frames are skipped.  At 30 FPS the nominal interval is ~0.033s;
    /// we allow up to 2x that to accommodate occasional slow frames while
    /// still clamping large gaps.
    const MAX_DT: f64 = 0.066;

    pub fn tick(&mut self) {
        let now = Instant::now();
        let raw_dt = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        // Clamp dt so that a long gap (frame skip, slow draw, debugger pause)
        // never produces a visible jump in auto-rotation.
        let dt = raw_dt.min(Self::MAX_DT);
        if self.auto_rotate {
            // Auto-rotate around screen Y axis (camera-local).
            let inc = rot_y_mat(-Self::AUTO_ROTATE_SPEED * dt);
            self.mat = mul(&inc, &self.mat);
        }
    }

    /// Reset the internal tick timer without applying any rotation.
    /// Call this when skipping frames so the next real tick starts from a
    /// fresh baseline rather than accumulating all the skipped time.
    pub fn reset_tick_timer(&mut self) {
        self.last_tick = Instant::now();
    }

    /// Project a 3D point to 2D using the accumulated rotation matrix + orthographic projection.
    pub fn project(&self, x: f64, y: f64, z: f64) -> Projected {
        let x3 = self.mat[0][0] * x + self.mat[0][1] * y + self.mat[0][2] * z;
        let y3 = self.mat[1][0] * x + self.mat[1][1] * y + self.mat[1][2] * z;
        let z2 = self.mat[2][0] * x + self.mat[2][1] * y + self.mat[2][2] * z;

        // Apply zoom and pan (orthographic projection)
        Projected {
            x: -x3 * self.zoom + self.pan_x,
            y: y3 * self.zoom + self.pan_y,
            z: z2,
        }
    }

    /// Pre-compute the rotation matrix for the current frame.
    ///
    /// Call this once per frame and then use [`ProjectionCache::project()`]
    /// and [`ProjectionCache::rotate_normal()`] for all per-vertex work.
    pub fn projection_cache(&self) -> ProjectionCache {
        ProjectionCache {
            mat: self.mat,
            zoom: self.zoom,
            pan_x: self.pan_x,
            pan_y: self.pan_y,
        }
    }

    /// Extract approximate Euler angles (X, Y, Z) from the rotation matrix.
    ///
    /// Used for state-file export to maintain backward-compatible JSON output.
    /// The extraction follows the ZYX convention matching the original Euler
    /// angle order used in the old camera code.
    ///
    /// When cos_y is near zero (gimbal lock), we set rot_x to zero and
    /// attribute the full rotation to rot_y +/- rot_z.
    pub fn euler_angles(&self) -> (f64, f64, f64) {
        // mat = Rz * Ry * Rx  (ZYX convention)
        // m[2][0] = -sin(y)
        let m = &self.mat;
        let sin_y = -m[2][0];
        let cos_y = m[0][0].hypot(m[1][0]);

        if cos_y > 1e-6 {
            let rot_x = m[2][1].atan2(m[2][2]);
            let rot_y = sin_y.atan2(cos_y);
            let rot_z = m[1][0].atan2(m[0][0]);
            (rot_x, rot_y, rot_z)
        } else {
            // Gimbal lock: cos(y) ~ 0
            let rot_x = 0.0;
            let rot_y = if sin_y > 0.0 {
                std::f64::consts::FRAC_PI_2
            } else {
                -std::f64::consts::FRAC_PI_2
            };
            let rot_z = (-m[1][2]).atan2(m[1][1]);
            (rot_x, rot_y, rot_z)
        }
    }

    /// Read-only access to the rotation matrix (for serialization / tests).
    pub fn rotation_matrix(&self) -> &Mat3 {
        &self.mat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_identity_negates_x() {
        // With identity rotation (all angles zero) and unit zoom,
        // a point at positive world-X should project to negative screen-X.
        // This ensures a right-handed coordinate system so L-amino acids
        // are not rendered as their mirror-image D-amino acids.
        let cam = Camera::default();
        let p = cam.project(1.0, 0.0, 0.0);
        assert!(
            p.x < 0.0,
            "positive world-X should project to negative screen-X, got {}",
            p.x
        );
        assert!(
            (p.y).abs() < 1e-12,
            "Y should be zero for a point on the X axis"
        );
    }

    #[test]
    fn project_identity_preserves_y() {
        // Y axis should pass through without negation.
        let cam = Camera::default();
        let p = cam.project(0.0, 1.0, 0.0);
        assert!(
            p.y > 0.0,
            "positive world-Y should project to positive screen-Y, got {}",
            p.y
        );
        assert!(
            (p.x).abs() < 1e-12,
            "X should be zero for a point on the Y axis"
        );
    }

    #[test]
    fn project_respects_zoom_and_pan() {
        let mut cam = Camera::default();
        cam.zoom = 2.0;
        cam.pan_x = 5.0;
        cam.pan_y = 3.0;
        let p = cam.project(1.0, 1.0, 0.0);
        // x = -1.0 * 2.0 + 5.0 = 3.0
        assert!((p.x - 3.0).abs() < 1e-12, "expected x=3.0, got {}", p.x);
        // y = 1.0 * 2.0 + 3.0 = 5.0
        assert!((p.y - 5.0).abs() < 1e-12, "expected y=5.0, got {}", p.y);
    }

    #[test]
    fn rotate_y_produces_screen_rotation() {
        // rotate_y(+1) should produce a left-multiply with a negative Y rotation,
        // which means the resulting matrix element mat[2][0] becomes positive
        // (i.e. sin_y > 0).  We verify the matrix changed from identity.
        let mut cam = Camera::default();
        cam.rotate_y(1.0);
        let m = cam.rotation_matrix();
        // After rotate_y(+1), mat = Ry(-0.1) * I = Ry(-0.1)
        // m[2][0] = -sin(-0.1) = sin(0.1) > 0
        assert!(
            m[2][0] > 0.0,
            "rotate_y(+1) should produce positive mat[2][0], got {}",
            m[2][0]
        );
    }

    #[test]
    fn rotate_z_produces_screen_rotation() {
        // rotate_z(+1) should produce a left-multiply with a negative Z rotation.
        // mat = Rz(-0.1) * I = Rz(-0.1)
        // m[1][0] = sin(-0.1) < 0
        let mut cam = Camera::default();
        cam.rotate_z(1.0);
        let m = cam.rotation_matrix();
        assert!(
            m[1][0] < 0.0,
            "rotate_z(+1) should produce negative mat[1][0], got {}",
            m[1][0]
        );
    }

    #[test]
    fn tick_clamps_large_dt() {
        // Simulate a long gap (e.g. frame skip) by creating a camera whose
        // last_tick is far in the past, then calling tick().  The rotation
        // should be clamped to MAX_DT worth of movement.
        let mut cam = Camera::default();
        cam.auto_rotate = true;
        // Manually set last_tick 500ms in the past (way more than MAX_DT)
        cam.last_tick = Instant::now() - std::time::Duration::from_millis(500);
        cam.tick();
        // Expected rotation ≈ AUTO_ROTATE_SPEED * MAX_DT = 0.6 * 0.066 = 0.0396
        // The matrix is Ry(angle) where angle = -AUTO_ROTATE_SPEED * dt (clamped).
        // |angle| <= 0.0396.  For a Y rotation: mat[2][0] = -sin(angle) ≈ angle.
        // Actually: auto_rotate uses Ry(-speed*dt), so mat[2][0] = sin(speed*dt).
        let expected_max_angle = Camera::AUTO_ROTATE_SPEED * Camera::MAX_DT;
        let actual_angle = cam.rotation_matrix()[2][0].asin();
        assert!(
            actual_angle.abs() <= expected_max_angle + 0.001,
            "rotation should be clamped to at most {:.4} rad, got {:.4}",
            expected_max_angle,
            actual_angle
        );
    }

    #[test]
    fn reset_tick_timer_prevents_jump() {
        // After reset_tick_timer(), the next tick() should see near-zero dt
        // and apply negligible rotation.
        let mut cam = Camera::default();
        cam.auto_rotate = true;
        // Set last_tick far in the past
        cam.last_tick = Instant::now() - std::time::Duration::from_secs(2);
        // Reset timer (as the main loop does during frame skips)
        cam.reset_tick_timer();
        // Immediately tick -- dt should be ~0
        cam.tick();
        // Matrix should be nearly identity
        let m = cam.rotation_matrix();
        assert!(
            (m[0][0] - 1.0).abs() < 0.001,
            "matrix should be near identity after reset + immediate tick"
        );
    }

    #[test]
    fn projection_cache_matches_camera_project() {
        // ProjectionCache::project() must produce identical results to
        // Camera::project() for any rotation/zoom/pan combination.
        let mut cam = Camera::default();
        // Apply some rotations to get a non-trivial matrix
        cam.rotate_x(0.7);
        cam.rotate_y(-1.2);
        cam.rotate_z(0.3);
        cam.zoom = 2.5;
        cam.pan_x = 10.0;
        cam.pan_y = -5.0;

        let cache = cam.projection_cache();
        let points = [
            (1.0, 2.0, 3.0),
            (-4.0, 0.5, -1.0),
            (0.0, 0.0, 0.0),
            (100.0, -200.0, 50.0),
        ];
        for (x, y, z) in points {
            let a = cam.project(x, y, z);
            let b = cache.project(x, y, z);
            assert!((a.x - b.x).abs() < 1e-12, "x mismatch for ({x},{y},{z})");
            assert!((a.y - b.y).abs() < 1e-12, "y mismatch for ({x},{y},{z})");
            assert!((a.z - b.z).abs() < 1e-12, "z mismatch for ({x},{y},{z})");
        }
    }

    #[test]
    fn projection_cache_rotate_normal_matches() {
        // Verify rotate_normal matches the matrix-based rotation.
        let mut cam = Camera::default();
        cam.rotate_x(0.5);
        cam.rotate_y(-0.8);
        cam.rotate_z(1.1);

        let cache = cam.projection_cache();
        let m = cam.rotation_matrix();

        let normals = [
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.0, 1.0),
            (0.577, 0.577, 0.577),
        ];
        for (nx, ny, nz) in normals {
            let result = cache.rotate_normal(nx, ny, nz);

            // Expected: mat * [nx, ny, nz]^T
            let expected_x = m[0][0] * nx + m[0][1] * ny + m[0][2] * nz;
            let expected_y = m[1][0] * nx + m[1][1] * ny + m[1][2] * nz;
            let expected_z = m[2][0] * nx + m[2][1] * ny + m[2][2] * nz;

            assert!(
                (result[0] - expected_x).abs() < 1e-12,
                "nx mismatch: got {} expected {}",
                result[0],
                expected_x
            );
            assert!(
                (result[1] - expected_y).abs() < 1e-12,
                "ny mismatch: got {} expected {}",
                result[1],
                expected_y
            );
            assert!(
                (result[2] - expected_z).abs() < 1e-12,
                "nz mismatch: got {} expected {}",
                result[2],
                expected_z
            );
        }
    }

    #[test]
    fn screen_space_rotation_preserves_axis() {
        // After rotating 90 degrees around screen-Y, pressing j/k (rotate_x)
        // should rotate around the screen's horizontal axis, not the world X
        // axis.
        //
        // With the old world-space Euler angles, after Ry(90deg), an Rx rotation
        // would rotate around world-X, moving the point (0,0,1) in Y.
        // With screen-space rotation, Rx after Ry rotates around screen-X, so
        // a point that lies on the screen-X axis stays on that axis.
        //
        // Detailed math:
        //   mat = Rx(pi/2) * Ry(-pi/2)
        //       = [[1,0,0],[0,0,-1],[0,1,0]] * [[0,0,-1],[0,1,0],[1,0,0]]
        //       = [[0,0,-1],[-1,0,0],[0,1,0]]
        //
        //   Project (0,0,1): x3=-1, y3=0, z2=0
        //   screen.x = -(-1)*1 + 0 = 1 (positive)
        //   screen.y = 0*1 + 0 = 0
        //
        // Compare with old world-space Euler (Rx=pi/2, Ry=-pi/2, Rz=0):
        //   mat = Rz * Ry * Rx (Euler order)
        //   y1 = 0*0 - 1*1 = -1,  z1 = 0*1 + 1*0 = 0
        //   x2 = 0*0 + 0*(-1) = 0, z2 = -0*0 + 0*1 = 0  (sin_y=-1, cos_y=0)
        //   x3 = 0*1 - (-1)*0 = 0, y3 = 0*0 + (-1)*1 = -1
        //   screen.x = -(0) = 0, screen.y = -1
        //
        // The key difference: screen-space gives y=0 (point stays on screen-X
        // axis), while world-space Euler gives y=-1 (point moved off axis).
        let mut cam = Camera::default();

        // Rotate 90 degrees around screen-Y
        let inc = rot_y_mat(-std::f64::consts::FRAC_PI_2);
        cam.mat = mul(&inc, &cam.mat);

        // Now rotate around screen-X by 90 degrees.
        let inc_x = rot_x_mat(std::f64::consts::FRAC_PI_2);
        cam.mat = mul(&inc_x, &cam.mat);

        // Point (0,0,1) should project with y=0 (stayed on screen-X axis).
        let p = cam.project(0.0, 0.0, 1.0);
        assert!(
            p.x > 0.5,
            "after Ry(-90) + Rx(90), point (0,0,1) should have large positive screen-x, got {}",
            p.x
        );
        assert!(
            p.y.abs() < 1e-10,
            "screen-space rotation: point (0,0,1) should stay on screen-X axis (y=0), got {}",
            p.y
        );
    }

    #[test]
    fn euler_extraction_roundtrip() {
        // For small angles, euler extraction should recover approximately
        // the same angles that were applied.
        let mut cam = Camera::default();
        let rx = 0.3;
        let ry = -0.5;
        let rz = 0.2;

        // Apply rotations in ZYX order: mat = Rz * Ry * Rx
        let mat_rx = rot_x_mat(rx);
        let mat_ry = rot_y_mat(ry);
        let mat_rz = rot_z_mat(rz);
        cam.mat = mul(&mat_rz, &mul(&mat_ry, &mat_rx));

        let (ex, ey, ez) = cam.euler_angles();
        assert!((ex - rx).abs() < 1e-10, "rot_x: expected {rx}, got {ex}");
        assert!((ey - ry).abs() < 1e-10, "rot_y: expected {ry}, got {ey}");
        assert!((ez - rz).abs() < 1e-10, "rot_z: expected {rz}, got {ez}");
    }

    #[test]
    fn reset_clears_rotation() {
        let mut cam = Camera::default();
        cam.rotate_x(5.0);
        cam.rotate_y(3.0);
        cam.rotate_z(2.0);
        cam.zoom = 10.0;
        cam.pan_x = 100.0;
        cam.pan_y = 200.0;
        cam.reset();
        // After reset, matrix should be identity
        let m = cam.rotation_matrix();
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (m[i][j] - expected).abs() < 1e-15,
                    "mat[{i}][{j}] should be {expected}, got {}",
                    m[i][j]
                );
            }
        }
        assert!((cam.zoom - 1.0).abs() < 1e-15);
        assert!((cam.pan_x).abs() < 1e-15);
        assert!((cam.pan_y).abs() < 1e-15);
    }

    #[test]
    fn matrix_mul_identity() {
        let id = identity();
        let a = rot_x_mat(0.5);
        let result = mul(&a, &id);
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (result[i][j] - a[i][j]).abs() < 1e-15,
                    "A*I should equal A"
                );
            }
        }
    }
}
