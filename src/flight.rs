//! Local rigid-body flight, with forgiving trim/damping and no external physics engine.

use glam::{Mat3, Quat, Vec2, Vec3};

pub const FIXED_DT: f32 = 1. / 120.;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlightTuning {
    pub mass: f32,
    pub inertia: Vec3,
    pub wing_area: f32,
    pub thrust: f32,
    pub angular_response: f32,
    pub side_damping: f32,
    pub max_rates: Vec3,
}

impl Default for FlightTuning {
    fn default() -> Self {
        Self {
            mass: 9000.,
            inertia: Vec3::new(35000., 65000., 90000.),
            wing_area: 28.,
            thrust: 32000.,
            angular_response: 5.,
            side_damping: 1.5,
            max_rates: Vec3::new(0.85, 0.55, 1.9),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RigidBody {
    pub position: Vec3,
    pub velocity: Vec3,
    pub orientation: Quat,
    /// Angular velocity in local body coordinates: right, up, backward.
    pub angular_velocity: Vec3,
}

impl RigidBody {
    pub fn new(position: Vec3, orientation: Quat) -> Self {
        Self {
            position,
            orientation,
            velocity: orientation * -Vec3::Z * 170.,
            angular_velocity: Vec3::ZERO,
        }
    }
    pub fn forward(self) -> Vec3 {
        self.orientation * -Vec3::Z
    }
    pub fn up(self) -> Vec3 {
        self.orientation * Vec3::Y
    }

    /// Render-only interpolation between adjacent authoritative physics snapshots.
    pub fn interpolate(self, current: Self, alpha: f32) -> Self {
        let alpha = alpha.clamp(0., 1.);
        Self {
            position: self.position.lerp(current.position, alpha),
            velocity: self.velocity.lerp(current.velocity, alpha),
            orientation: self
                .orientation
                .slerp(current.orientation, alpha)
                .normalize(),
            angular_velocity: self.angular_velocity.lerp(current.angular_velocity, alpha),
        }
    }

    pub fn step(&mut self, input: Controls, tuning: FlightTuning, dt: f32) {
        let forward = self.forward();
        let up = self.up();
        let right = self.orientation * Vec3::X;
        let speed = self.velocity.length();
        let local_velocity = self.orientation.conjugate() * self.velocity;
        let airspeed = (-local_velocity.z).max(0.);
        let aoa = (-local_velocity.y).atan2(airspeed.max(1.));
        let q = 0.5 * 1.225 * airspeed * airspeed;
        // Automatic trim makes level flight approachable; AoA still produces real lift/drag.
        let trim = (tuning.mass * 9.81 / (q * tuning.wing_area).max(1.)).clamp(0.1, 1.35);
        let stall = (1. - ((aoa.abs() - 0.40).max(0.) / 0.8)).clamp(0.2, 1.);
        let cl = (trim + aoa * 3.2 + input.pitch * 0.20).clamp(-1.2, 1.8) * stall;
        let direction = self.velocity.normalize_or_zero();
        let lift_direction = (up - direction * up.dot(direction)).normalize_or_zero();
        let lift = lift_direction * q * tuning.wing_area * cl;
        let drag = -direction
            * 0.5
            * 1.225
            * speed
            * speed
            * tuning.wing_area
            * (0.030 + 0.045 * cl * cl + 0.12 * aoa * aoa);
        let damping = -right * local_velocity.x * tuning.mass * tuning.side_damping
            - up * local_velocity.y * tuning.mass * 0.16;
        let force = forward * tuning.thrust * input.throttle.clamp(0., 1.) + lift + drag + damping
            - Vec3::Y * tuning.mass * 9.81;
        self.velocity += force / tuning.mass * dt;
        // A soft overspeed drag safeguard rather than an instantaneous speed clamp.
        if speed > 340. {
            self.velocity *= (-((speed - 340.) * 0.006) * dt).exp();
        }
        self.position += self.velocity * dt;

        let authority = 0.35 + 0.65 * (airspeed / 110.).clamp(0., 1.);
        let bank = (-right.y).atan2(up.y);
        let coordinated_yaw = if up.y > 0.15 {
            -9.81 * bank.clamp(-1.15, 1.15).tan() / airspeed.max(65.)
        } else {
            0.
        };
        let desired =
            Vec3::new(input.pitch, -input.yaw, -input.roll) * tuning.max_rates * authority
                + Vec3::Y * coordinated_yaw;
        let momentum = tuning.inertia * self.angular_velocity;
        let torque = tuning.inertia * (desired - self.angular_velocity) * tuning.angular_response;
        self.angular_velocity +=
            (torque - self.angular_velocity.cross(momentum)) / tuning.inertia * dt;
        self.orientation =
            (self.orientation * Quat::from_scaled_axis(self.angular_velocity * dt)).normalize();
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Controls {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub throttle: f32,
    pub fire: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            pitch: 0.,
            yaw: 0.,
            roll: 0.,
            throttle: 0.55,
            fire: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum FlightKey {
    W,
    S,
    A,
    D,
    Q,
    E,
    Up,
    Down,
    Left,
    Right,
    Fire,
}

#[derive(Default)]
pub struct FlightInput {
    held: u16,
    pub mouse: Vec2,
}

impl FlightInput {
    pub fn set(&mut self, key: FlightKey, pressed: bool) {
        let mask = 1 << key as u16;
        if pressed {
            self.held |= mask;
        } else {
            self.held &= !mask;
        }
    }
    fn down(&self, key: FlightKey) -> f32 {
        if self.held & (1 << key as u16) != 0 {
            1.
        } else {
            0.
        }
    }
    pub fn clear(&mut self) {
        self.held = 0;
        self.mouse = Vec2::ZERO;
    }
    pub fn mouse_motion(&mut self, delta: Vec2, viewport: [u32; 2], scheme: usize) {
        let scale = (viewport[0] as f32 / 1280.)
            .min(viewport[1] as f32 / 720.)
            .max(0.1);
        if scheme == 2 {
            self.mouse = (self.mouse + delta / (180. * scale)).clamp_length_max(1.);
        } else if scheme == 3 {
            self.mouse = (self.mouse + delta / (Vec2::new(520., 280.) * scale))
                .clamp(Vec2::splat(-1.), Vec2::ONE);
        }
    }
    pub fn controls(&self, scheme: usize, body: RigidBody, aim: Vec3) -> Controls {
        let yaw = self.down(FlightKey::D) - self.down(FlightKey::A);
        let key_roll = self.down(FlightKey::E) - self.down(FlightKey::Q);
        let arrow_roll = self.down(FlightKey::Right) - self.down(FlightKey::Left);
        let arrow_pitch = self.down(FlightKey::Up) - self.down(FlightKey::Down);
        let mut controls = match scheme {
            0 => Controls {
                pitch: -arrow_pitch,
                yaw,
                roll: arrow_roll,
                ..Default::default()
            },
            1 => Controls {
                pitch: arrow_pitch,
                yaw,
                roll: (key_roll + arrow_roll).clamp(-1., 1.),
                ..Default::default()
            },
            2 => Controls {
                pitch: self.mouse.y,
                yaw,
                roll: (self.mouse.x + key_roll).clamp(-1., 1.),
                ..Default::default()
            },
            _ => {
                let mut controls = steer_toward(body, aim);
                if yaw != 0. {
                    controls.yaw = yaw;
                }
                if key_roll != 0. {
                    controls.roll = key_roll;
                }
                controls
            }
        };
        controls.throttle = if self.down(FlightKey::W) > 0. {
            1.
        } else if self.down(FlightKey::S) > 0. {
            0.10
        } else {
            0.55
        };
        controls.fire = self.down(FlightKey::Fire) > 0.;
        controls
    }
}

pub fn steer_toward(body: RigidBody, direction: Vec3) -> Controls {
    let forward = direction.try_normalize().unwrap_or(body.forward());
    // Preserve the current upright/inverted hemisphere so manual roll remains useful.
    let preferred_up = if body.up().y >= 0. { Vec3::Y } else { -Vec3::Y };
    let right = forward
        .cross(preferred_up)
        .try_normalize()
        .unwrap_or(body.orientation * Vec3::X);
    let up = right.cross(forward).normalize();
    let local = body.orientation.conjugate() * forward;
    let bank = local.x.atan2(-local.z).clamp(-1., 1.) * 0.8;
    let desired =
        Quat::from_mat3(&Mat3::from_cols(right, up, -forward)) * Quat::from_rotation_z(-bank);
    let mut error = body.orientation.conjugate() * desired;
    if error.w < 0. {
        error = -error;
    }
    let axis = error.to_scaled_axis();
    Controls {
        pitch: (axis.x * 2.).clamp(-1., 1.),
        yaw: (-axis.y * 1.5).clamp(-1., 1.),
        roll: (-axis.z * 2.).clamp(-1., 1.),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pose_interpolation_uses_shortest_rotation_and_clamps_endpoints() {
        let mut previous = RigidBody::new(Vec3::ZERO, Quat::from_rotation_y(170_f32.to_radians()));
        previous.velocity = Vec3::ZERO;
        let mut current = previous;
        current.position = Vec3::X * 10.;
        current.orientation = Quat::from_rotation_y(-170_f32.to_radians());
        let midpoint = previous.interpolate(current, 0.5);
        assert!((midpoint.position.x - 5.).abs() < 1e-5);
        assert!(
            midpoint
                .orientation
                .dot(Quat::from_rotation_y(std::f32::consts::PI))
                .abs()
                > 0.9999
        );
        assert_eq!(
            previous.interpolate(current, -1.).position,
            previous.position
        );
        assert_eq!(previous.interpolate(current, 2.).position, current.position);
    }
    #[test]
    fn neutral_flight_is_stable_and_finite() {
        let mut body = RigidBody::new(Vec3::Y * 2000., Quat::IDENTITY);
        for _ in 0..120 * 60 {
            body.step(Controls::default(), FlightTuning::default(), FIXED_DT);
        }
        assert!(body.position.is_finite() && body.velocity.is_finite());
        assert!(
            body.position.y > 1500. && body.position.y < 2500.,
            "{:?}",
            body.position
        );
        assert!(
            (90. ..300.).contains(&body.velocity.length()),
            "{:?}",
            body.velocity
        );
        assert!((body.orientation.length() - 1.).abs() < 0.0001);
    }
    #[test]
    fn controls_release_and_all_four_schemes_match_the_document() {
        let mut input = FlightInput::default();
        let body = RigidBody::new(Vec3::ZERO, Quat::IDENTITY);
        input.set(FlightKey::Down, true);
        assert_eq!(input.controls(0, body, -Vec3::Z).pitch, 1.);
        assert_eq!(input.controls(1, body, -Vec3::Z).pitch, -1.);
        input.set(FlightKey::Down, false);
        input.set(FlightKey::A, true);
        assert_eq!(input.controls(0, body, -Vec3::Z).yaw, -1.);
        input.set(FlightKey::W, true);
        assert_eq!(input.controls(1, body, -Vec3::Z).throttle, 1.);
        input.clear();
        assert_eq!(input.controls(0, body, -Vec3::Z).throttle, 0.55);
        assert_eq!(input.controls(0, body, -Vec3::Z).roll, 0.);
        input.mouse = Vec2::new(0.4, 0.3);
        assert_eq!(input.controls(2, body, -Vec3::Z).pitch, 0.3);
        input.set(FlightKey::E, true);
        input.set(FlightKey::D, true);
        let controls = input.controls(3, body, -Vec3::Z);
        assert_eq!((controls.roll, controls.yaw), (1., 1.));
    }
    #[test]
    fn pitch_roll_and_yaw_have_expected_signs() {
        for (controls, axis) in [
            (
                Controls {
                    pitch: 1.,
                    ..Default::default()
                },
                0,
            ),
            (
                Controls {
                    yaw: 1.,
                    ..Default::default()
                },
                1,
            ),
            (
                Controls {
                    roll: 1.,
                    ..Default::default()
                },
                2,
            ),
        ] {
            let mut body = RigidBody::new(Vec3::Y * 1000., Quat::IDENTITY);
            for _ in 0..60 {
                body.step(controls, FlightTuning::default(), FIXED_DT);
            }
            match axis {
                0 => assert!(body.forward().y > 0.),
                1 => assert!(body.forward().x > 0.),
                _ => assert!(body.up().x > 0.),
            }
        }
    }
}
