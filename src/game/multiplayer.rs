//! Host fixed ticks, input batching, client prediction/reconciliation, and remote interpolation.

use super::{net_types::*, *};
use crate::{
    config::SyncTuning,
    networking::{NetworkEvent, Request},
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy)]
struct Peer {
    op: u64,
    id: u64,
    host: bool,
}
#[derive(Default)]
pub struct Multiplayer {
    peer: Option<Peer>,
    game: Option<Running>,
    requests: VecDeque<Request>,
}

#[derive(Default)]
struct InputQueue {
    frames: BTreeMap<u32, InputFrame>,
    ack: u32,
    last: Option<InputFrame>,
    stale: f32,
    warmup: usize,
    started: bool,
    last_action: u32,
}
impl InputQueue {
    fn insert(&mut self, first: u32, frames: &[InputFrame], s: SyncTuning) -> Result<(), String> {
        if first == 0
            || frames.is_empty()
            || frames.len() > 64
            || first.checked_add(frames.len() as u32).is_none()
        {
            return Err("Invalid input batch".into());
        }
        for (offset, &frame) in frames.iter().enumerate() {
            validate_input(frame)?;
            let seq = first + offset as u32;
            if seq > self.ack
                && seq <= self.ack.saturating_add(1024)
                && self.frames.len() < s.max_input_queue
            {
                self.frames.entry(seq).or_insert(frame);
            }
        }
        if !self.started && !self.frames.is_empty() {
            self.warmup = s.input_buffer_ticks;
            self.started = true;
        }
        Ok(())
    }
    fn consume(&mut self, dt: f32, s: SyncTuning) -> (Controls, u8) {
        // A stalled connection must not leave a permanent half-second backlog.
        // Skipping old commands changes no physics rate: only one sample is used below.
        if self.frames.len() > s.max_input_queue / 2 {
            let keep = ((1. / dt) / s.input_hz as f32).ceil() as usize + s.input_buffer_ticks;
            while self.frames.len() > keep {
                self.frames.pop_first();
            }
        }
        if self.warmup > 0 {
            self.warmup -= 1;
            return (Controls::default(), 0);
        }
        if let Some((seq, frame)) = self.frames.pop_first() {
            self.ack = seq;
            self.last = Some(frame);
            self.stale = 0.;
            return (decode_input(frame), frame[5] as u8);
        }
        self.stale += dt;
        if self.stale < s.input_timeout {
            if let Some(frame) = self.last {
                let mut c = decode_input(frame);
                c.fire = false;
                return (c, frame[5] as u8);
            }
        }
        (Controls::default(), self.last.map_or(0, |f| f[5] as u8))
    }
}

#[derive(Clone, Copy)]
struct Predicted {
    seq: u32,
    life: u32,
    controls: Controls,
}
#[derive(Clone, Copy)]
struct PoseSample {
    tick: u64,
    body: RigidBody,
    life: u32,
}

struct Running {
    setup: MatchSetup,
    players: Vec<PlayerSlot>,
    authority: bool,
    local: usize,
    tick: u64,
    clock: f64,
    accumulator: f64,
    pulse: f64,
    input_seq: u32,
    last_ack: u32,
    last_packet: u64,
    inputs: Vec<InputQueue>,
    sent: VecDeque<(u32, InputFrame)>,
    predicted: VecDeque<Predicted>,
    tracks: Vec<VecDeque<PoseSample>>,
    corrections: Vec<(Vec3, Quat)>,
    tombstones: BTreeMap<u32, u64>,
    desired_weapon: u8,
    awaiting_spawn: bool,
    awaiting_action: u32,
}

impl Multiplayer {
    pub fn epoch(&self) -> Option<u32> {
        self.game.as_ref().map(|g| g.setup.epoch)
    }
    pub fn is_active(&self) -> bool {
        self.game.is_some()
    }
    pub fn awaiting_spawn(&self) -> bool {
        self.game.as_ref().is_some_and(|g| g.awaiting_spawn)
    }
    pub fn take_requests(&mut self) -> VecDeque<Request> {
        std::mem::take(&mut self.requests)
    }
    pub fn clear(&mut self) {
        self.peer = None;
        self.game = None;
        self.requests.clear();
    }
    pub fn swap_weapon(&mut self) {
        if let Some(g) = &mut self.game {
            g.desired_weapon ^= 1;
        }
    }
    pub fn note_request(&mut self, request: &Request) {
        if matches!(request, Request::Leave) {
            self.clear();
        }
        if let Request::Action {
            action: GameAction::Spawn,
            action_id,
            ..
        } = request
        {
            if let Some(g) = &mut self.game {
                g.awaiting_spawn = true;
                g.awaiting_action = *action_id;
                g.desired_weapon = 0;
            }
        }
    }
    pub fn handle(
        &mut self,
        event: &NetworkEvent,
        world: &mut World,
        config: &GameConfig,
    ) -> Result<(), String> {
        if let NetworkEvent::Connected { op, info, .. } = event {
            self.game = None;
            self.peer = Some(Peer {
                op: *op,
                id: info.player_id,
                host: info.host,
            });
            return Ok(());
        }
        let Some(peer) = self.peer else {
            return Ok(());
        };
        if event.operation().is_some_and(|op| op != peer.op) {
            return Ok(());
        }
        match event {
            NetworkEvent::Prepare {
                epoch,
                players,
                capacity,
                free_for_all,
                ..
            } if peer.host => {
                validate_players(players, *capacity, peer.id)?;
                let mut rules = config.rules;
                rules.max_projectiles = rules.max_projectiles.min(MAX_NETWORK_PROJECTILES);
                rules.bot_count = rules.bot_count.min(MAX_ACTORS - *capacity);
                self.requests.push_back(Request::Begin {
                    op: peer.op,
                    setup: MatchSetup {
                        epoch: *epoch,
                        capacity: *capacity,
                        rules,
                        flight: config.flight,
                        sync: config.sync,
                        level_signature: world.terrain.signature(),
                        free_for_all: *free_for_all,
                    },
                });
            }
            NetworkEvent::GameStarted { setup, players, .. } => {
                validate_players(players, setup.capacity, peer.id)?;
                let mut check = config.clone();
                check.rules = setup.rules;
                check.flight = setup.flight;
                check.sync = setup.sync;
                check.validate().map_err(|e| e.to_string())?;
                if setup.epoch == 0
                    || setup.capacity + setup.rules.bot_count > MAX_ACTORS
                    || setup.rules.max_projectiles > MAX_NETWORK_PROJECTILES
                    || setup.level_signature != world.terrain.signature()
                {
                    return Err("Host level or gameplay configuration is incompatible".into());
                }
                let local = players.iter().find(|p| p.id == peer.id).unwrap().slot;
                world.configure_network(setup, local, peer.host);
                let count = world.actors.len();
                self.game = Some(Running {
                    setup: setup.clone(),
                    players: players.clone(),
                    authority: peer.host,
                    local,
                    tick: 0,
                    clock: 0.,
                    accumulator: 0.,
                    pulse: 0.,
                    input_seq: 0,
                    last_ack: 0,
                    last_packet: 0,
                    inputs: (0..setup.capacity).map(|_| InputQueue::default()).collect(),
                    sent: VecDeque::new(),
                    predicted: VecDeque::new(),
                    tracks: (0..count).map(|_| VecDeque::new()).collect(),
                    corrections: vec![(Vec3::ZERO, Quat::IDENTITY); count],
                    tombstones: BTreeMap::new(),
                    desired_weapon: 0,
                    awaiting_spawn: false,
                    awaiting_action: 0,
                });
            }
            NetworkEvent::Participants { epoch, players, .. } => {
                if let Some(g) = &mut self.game {
                    if *epoch != g.setup.epoch {
                        return Ok(());
                    }
                    validate_players(players, g.setup.capacity, peer.id)?;
                    for old in &g.players {
                        if !players.iter().any(|p| p.id == old.id) {
                            world.deactivate(old.slot);
                            g.inputs[old.slot] = InputQueue::default();
                            g.tracks[old.slot].clear();
                        }
                    }
                    g.players = players.clone();
                }
            }
            NetworkEvent::Inputs {
                epoch,
                player,
                first,
                frames,
                ..
            } => {
                if let Some(g) = &mut self.game {
                    if g.authority && *epoch == g.setup.epoch {
                        if let Some(p) = g.players.iter().find(|p| p.id == *player) {
                            g.inputs[p.slot].insert(*first, frames, g.setup.sync)?;
                        }
                    }
                }
            }
            NetworkEvent::State { epoch, packet, .. } => {
                if let Some(g) = &mut self.game {
                    if !g.authority && *epoch == g.setup.epoch {
                        g.receive(packet, world)?;
                    }
                }
            }
            NetworkEvent::Action {
                epoch,
                player,
                action_id,
                action,
                team,
                ..
            } => {
                if let Some(g) = &mut self.game {
                    if !g.authority || *epoch != g.setup.epoch {
                        return Ok(());
                    }
                    let Some(p) = g.players.iter().find(|p| p.id == *player) else {
                        return Ok(());
                    };
                    let slot = p.slot;
                    let result = if *action_id <= g.inputs[slot].last_action {
                        Err("Old action".into())
                    } else {
                        g.inputs[slot].last_action = *action_id;
                        match action {
                            GameAction::Spawn => world.spawn_network(slot, *team),
                            GameAction::Respawn => {
                                if world.actors[slot].alive() {
                                    world.destroy(slot, None);
                                }
                                Ok(())
                            }
                            GameAction::End if *player == peer.id => {
                                world.finished = true;
                                Ok(())
                            }
                            GameAction::End => Err("Only the host may end the match".into()),
                        }
                    };
                    if *player == peer.id && matches!(action, GameAction::Spawn) && result.is_ok() {
                        g.awaiting_spawn = false;
                    }
                    self.requests.push_back(Request::ActionResult {
                        op: peer.op,
                        epoch: g.setup.epoch,
                        player: *player,
                        action_id: *action_id,
                        accepted: result.is_ok(),
                        reason: result.err().unwrap_or_default(),
                    });
                }
            }
            NetworkEvent::ActionResult {
                epoch,
                accepted,
                action_id,
                ..
            } => {
                if let Some(g) = &mut self.game {
                    if *epoch == g.setup.epoch && *action_id == g.awaiting_action && !accepted {
                        g.awaiting_spawn = false;
                    }
                }
            }
            NetworkEvent::ReturnedLobby { .. } => {
                self.game = None;
                self.requests.clear();
            }
            NetworkEvent::Disconnected { .. } | NetworkEvent::WorkerStopped { .. } => self.clear(),
            _ => {}
        }
        Ok(())
    }
    pub fn advance(
        &mut self,
        seconds: f64,
        world: &mut World,
        input: &FlightInput,
        scheme: usize,
        aim: Vec3,
        controlling: bool,
    ) {
        let (Some(peer), Some(g)) = (self.peer, &mut self.game) else {
            return;
        };
        let seconds = seconds.clamp(0., 0.1);
        g.accumulator += seconds;
        g.pulse += seconds;
        let dt = g.setup.rules.fixed_dt();
        let fixed_seconds = 1. / g.setup.rules.physics_hz as f64;
        while g.accumulator + 1e-10 >= fixed_seconds {
            g.accumulator -= fixed_seconds;
            let controls = if controlling {
                input.controls(scheme, world.actors[g.local].body, aim)
            } else {
                Controls::default()
            };
            world.last_controls = controls;
            if g.authority {
                let mut inputs = vec![Some(Controls::default()); g.setup.capacity];
                for p in &g.players {
                    let (c, w) = if p.id == peer.id {
                        (controls, g.desired_weapon)
                    } else {
                        g.inputs[p.slot].consume(dt, g.setup.sync)
                    };
                    inputs[p.slot] = Some(c);
                    world.actors[p.slot].weapon = if w == 0 {
                        Weapon::Guns
                    } else {
                        Weapon::Missiles
                    };
                }
                if !world.finished {
                    world.step_with_controls(&inputs);
                }
                g.tick += 1;
            } else {
                g.input_seq = g.input_seq.saturating_add(1);
                let frame = encode_input(controls, g.desired_weapon);
                g.sent.push_back((g.input_seq, frame));
                while g.sent.len() > 64 {
                    g.sent.pop_front();
                }
                let actor = &mut world.actors[g.local];
                actor.previous_body = actor.body;
                if actor.alive() && !world.finished {
                    actor.body.step(decode_input(frame), world.tuning, dt);
                }
                g.predicted.push_back(Predicted {
                    seq: g.input_seq,
                    life: actor.life,
                    controls: decode_input(frame),
                });
                while g.predicted.len() > g.setup.sync.max_prediction_ticks {
                    g.predicted.pop_front();
                }
                g.clock += 1.;
                for slot in 0..world.actors.len() {
                    if slot != g.local {
                        if let Some(mut body) = sample_pose(
                            &g.tracks[slot],
                            g.clock - g.setup.sync.interpolation_seconds as f64 / dt as f64,
                            dt,
                            world.finished,
                            g.setup.sync.extrapolation_seconds,
                        ) {
                            let (offset, rotation) = &mut g.corrections[slot];
                            body.position += *offset;
                            body.orientation = (*rotation * body.orientation).normalize();
                            let decay = (-dt / 0.1).exp();
                            *offset *= decay;
                            *rotation = Quat::IDENTITY.slerp(*rotation, decay);
                            world.actors[slot].body = body;
                            world.actors[slot].previous_body = body;
                        }
                    }
                }
                let decay = (-dt / 0.1).exp();
                world.visual_offset *= decay;
                world.visual_rotation = Quat::IDENTITY.slerp(world.visual_rotation, decay);
                for p in &mut world.projectiles {
                    p.previous_position = p.position;
                    p.previous_velocity = p.velocity;
                    if !world.finished {
                        p.position += p.velocity * dt;
                        p.remaining -= dt;
                    }
                }
                world.projectiles.retain(|p| p.remaining > 0.);
                for e in &mut world.explosions {
                    e.age += dt;
                }
                world.explosions.retain(|e| e.age < 0.7);
            }
        }
        world.accumulator = g.accumulator;
        if world.actors[g.local].alive() {
            g.awaiting_spawn = false;
        }
        let hz = if g.authority {
            g.setup.sync.snapshot_hz
        } else {
            g.setup.sync.input_hz
        };
        if g.pulse >= 1. / hz as f64 {
            g.pulse %= 1. / hz as f64;
            if g.authority {
                let acks = g
                    .players
                    .iter()
                    .map(|p| (p.id, g.inputs[p.slot].ack))
                    .collect();
                self.requests.push_back(Request::State {
                    op: peer.op,
                    epoch: g.setup.epoch,
                    snapshot: world.snapshot(g.tick, acks),
                });
            } else if let Some(&(first, _)) = g.sent.front() {
                self.requests.push_back(Request::Inputs {
                    op: peer.op,
                    epoch: g.setup.epoch,
                    first,
                    frames: g.sent.iter().map(|(_, f)| *f).collect(),
                });
                let keep = g.setup.sync.redundancy_ticks;
                while g.sent.len() > keep {
                    g.sent.pop_front();
                }
            }
        }
    }
}

impl Running {
    fn receive(&mut self, packet: &StatePacket, world: &mut World) -> Result<(), String> {
        if packet.tick <= self.last_packet {
            return Ok(());
        }
        if packet.tick > u32::MAX as u64
            || packet.actors.len() > MAX_ACTORS
            || packet.projectiles.len() > 32
            || packet.explosions.len() > 8
            || packet.removed.len() > 64
            || packet.ack > self.input_seq
            || packet.ack < self.last_ack
            || !packet.actors.iter().any(|a| a[0] == self.local as i64)
        {
            return Err("Invalid host snapshot".into());
        }
        self.last_packet = packet.tick;
        self.last_ack = packet.ack;
        self.clock = self.clock.max(packet.tick as f64);
        world.elapsed = packet.elapsed_ms as f32 / 1000.;
        world.finished = packet.finished;
        for row in &packet.actors {
            let slot = usize::try_from(row[0]).map_err(|_| "Invalid actor")?;
            let old = world.actors.get(slot).ok_or("Invalid actor")?;
            let old_life = old.life;
            let old_alive = old.alive();
            let visible = world.render_body(slot);
            let (_, mut body) = world.import_actor(row)?;
            if slot == self.local {
                let life = world.actors[slot].life;
                self.predicted
                    .retain(|p| p.seq > packet.ack && p.life == life);
                if world.actors[slot].alive() && !packet.finished {
                    for p in &self.predicted {
                        body.step(p.controls, world.tuning, world.rules.fixed_dt());
                    }
                }
                let smooth = old_life == life
                    && old_alive
                    && world.actors[slot].alive()
                    && visible.position.distance(body.position) < 150.;
                world.visual_offset = if smooth {
                    visible.position - body.position
                } else {
                    Vec3::ZERO
                };
                world.visual_rotation = if smooth {
                    (visible.orientation * body.orientation.conjugate()).normalize()
                } else {
                    Quat::IDENTITY
                };
                world.actors[slot].body = body;
                world.actors[slot].previous_body = body;
            } else {
                let track = &mut self.tracks[slot];
                if track.back().is_some_and(|s| s.life != row[1] as u32) {
                    track.clear();
                }
                if track.is_empty() {
                    world.actors[slot].body = body;
                    world.actors[slot].previous_body = body;
                }
                track.push_back(PoseSample {
                    tick: packet.tick,
                    body,
                    life: row[1] as u32,
                });
                while track.len() > 8 {
                    track.pop_front();
                }
                let target = sample_pose(
                    track,
                    self.clock
                        - self.setup.sync.interpolation_seconds as f64
                            / world.rules.fixed_dt() as f64,
                    world.rules.fixed_dt(),
                    packet.finished,
                    self.setup.sync.extrapolation_seconds,
                )
                .unwrap();
                self.corrections[slot] = if old_life == row[1] as u32
                    && old_alive
                    && visible.position.distance(target.position) < 150.
                {
                    (
                        visible.position - target.position,
                        (visible.orientation * target.orientation.conjugate()).normalize(),
                    )
                } else {
                    (Vec3::ZERO, Quat::IDENTITY)
                };
            }
        }
        for id in &packet.removed {
            self.tombstones.insert(*id, packet.tick);
            world.projectiles.retain(|p| p.id != *id);
        }
        while self.tombstones.len() > 1024 {
            self.tombstones.pop_first();
        }
        for row in &packet.projectiles {
            if !self.tombstones.contains_key(&(row[0] as u32)) {
                world.import_projectile(row)?;
            }
        }
        for row in &packet.explosions {
            world.import_explosion(row)?;
        }
        world.update_local_stats();
        Ok(())
    }
}

fn validate_players(players: &[PlayerSlot], capacity: usize, local: u64) -> Result<(), String> {
    if !(2..=32).contains(&capacity)
        || players.len() > capacity
        || !players.iter().any(|p| p.id == local)
        || players
            .iter()
            .any(|p| p.id == 0 || p.slot >= capacity || p.name.len() > 24)
        || players.iter().enumerate().any(|(i, p)| {
            players[..i]
                .iter()
                .any(|q| q.id == p.id || q.slot == p.slot)
        })
    {
        return Err("Invalid player assignment".into());
    }
    Ok(())
}
pub fn validate_input(f: InputFrame) -> Result<(), String> {
    if f[..3].iter().any(|n| !(-1000..=1000).contains(n))
        || !(0..=1000).contains(&f[3])
        || !(0..=1).contains(&f[4])
        || !(0..=1).contains(&f[5])
    {
        return Err("Invalid flight input".into());
    }
    Ok(())
}
fn encode_input(c: Controls, weapon: u8) -> InputFrame {
    [
        (c.pitch.clamp(-1., 1.) * 1000.) as i16,
        (c.yaw.clamp(-1., 1.) * 1000.) as i16,
        (c.roll.clamp(-1., 1.) * 1000.) as i16,
        (c.throttle.clamp(0., 1.) * 1000.) as i16,
        i16::from(c.fire),
        weapon as i16,
    ]
}
fn decode_input(f: InputFrame) -> Controls {
    Controls {
        pitch: f[0] as f32 / 1000.,
        yaw: f[1] as f32 / 1000.,
        roll: f[2] as f32 / 1000.,
        throttle: f[3] as f32 / 1000.,
        fire: f[4] != 0,
    }
}
fn sample_pose(
    track: &VecDeque<PoseSample>,
    tick: f64,
    dt: f32,
    finished: bool,
    max_extrapolation: f32,
) -> Option<RigidBody> {
    let first = track.front()?;
    if tick <= first.tick as f64 {
        return Some(first.body);
    }
    for (a, b) in track.iter().zip(track.iter().skip(1)) {
        if tick <= b.tick as f64 {
            return Some(a.body.interpolate(
                b.body,
                ((tick - a.tick as f64) / (b.tick - a.tick) as f64) as f32,
            ));
        }
    }
    let last = track.back()?;
    let mut body = last.body;
    let seconds = if finished {
        0.
    } else {
        ((tick - last.tick as f64) * dt as f64).clamp(0., max_extrapolation as f64) as f32
    };
    body.position += body.velocity * seconds;
    body.orientation =
        (body.orientation * Quat::from_scaled_axis(body.angular_velocity * seconds)).normalize();
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{networking::SessionInfo, rendering::MeshData, terrain::Terrain};

    #[test]
    fn duplicate_chunks_do_not_add_ticks_and_stale_fire_stops() {
        let tuning = SyncTuning {
            input_buffer_ticks: 0,
            ..Default::default()
        };
        let mut queue = InputQueue::default();
        let frames = vec![[0, 0, 0, 550, 1, 0]; 6];
        queue.insert(1, &frames, tuning).unwrap();
        queue.insert(1, &frames, tuning).unwrap();
        assert_eq!(queue.frames.len(), 6);
        for seq in 1..=6 {
            assert!(queue.consume(1. / 120., tuning).0.fire);
            assert_eq!(queue.ack, seq);
        }
        assert!(!queue.consume(1. / 120., tuning).0.fire);
        queue.insert(1, &frames, tuning).unwrap();
        assert!(queue.frames.is_empty());
        assert_eq!(queue.ack, 6);
        // A later redundant chunk recovers samples from a lost middle packet.
        queue.insert(1, &vec![frames[0]; 18], tuning).unwrap();
        assert_eq!(queue.frames.len(), 12);
        assert!(queue.consume(1. / 120., tuning).0.fire);
        assert_eq!(
            queue.ack, 7,
            "one host tick cannot consume an entire recovered chunk"
        );
        assert!(queue.insert(u32::MAX, &frames, tuning).is_err());
    }

    #[test]
    fn remote_aircraft_interpolate_and_extrapolate_with_a_bound() {
        let mut first = RigidBody::new(Vec3::ZERO, Quat::IDENTITY);
        first.velocity = Vec3::X * 10.;
        let mut last = first;
        last.position = Vec3::X * 10.;
        last.orientation = Quat::from_rotation_y(std::f32::consts::PI);
        let track = VecDeque::from([
            PoseSample {
                tick: 0,
                body: first,
                life: 1,
            },
            PoseSample {
                tick: 120,
                body: last,
                life: 1,
            },
        ]);
        let halfway = sample_pose(&track, 60., 1. / 120., false, 0.75).unwrap();
        assert!((halfway.position.x - 5.).abs() < 0.001);
        assert!((halfway.orientation.length() - 1.).abs() < 0.001);
        assert!(
            (sample_pose(&track, 9999., 1. / 120., false, 0.75)
                .unwrap()
                .position
                .x
                - 17.5)
                .abs()
                < 0.001
        );
        assert_eq!(
            sample_pose(&track, 9999., 1. / 120., true, 0.75)
                .unwrap()
                .position,
            last.position
        );
    }

    #[test]
    fn owner_reconciles_unacked_inputs_and_new_life_clears_prediction() {
        let mut config = GameConfig::default();
        config.rules.bot_count = 0;
        config.rules.cover_count = 0;
        let terrain = || Terrain::new(&MeshData::default(), Mat4::IDENTITY);
        let mut host = World::configured(terrain(), &config);
        let mut replica = World::configured(terrain(), &config);
        let setup = MatchSetup {
            epoch: 7,
            capacity: 2,
            rules: config.rules,
            flight: config.flight,
            sync: config.sync,
            level_signature: host.terrain.signature(),
            free_for_all: false,
        };
        let players = vec![
            PlayerSlot {
                id: 1,
                slot: 0,
                name: "HOST".into(),
            },
            PlayerSlot {
                id: 2,
                slot: 1,
                name: "GUEST".into(),
            },
        ];
        let mut game = Multiplayer::default();
        game.handle(
            &NetworkEvent::Connected {
                op: 1,
                endpoint: "local".into(),
                info: SessionInfo {
                    name: "TEST".into(),
                    player_id: 2,
                    host: false,
                    max_players: 2,
                    map: "WORLD 01".into(),
                    mode: "TEAM DOGFIGHT".into(),
                    capabilities: vec![],
                    players: vec![],
                },
            },
            &mut replica,
            &config,
        )
        .unwrap();
        game.handle(
            &NetworkEvent::GameStarted {
                op: 1,
                setup: setup.clone(),
                players,
            },
            &mut replica,
            &config,
        )
        .unwrap();
        host.configure_network(&setup, 0, true);
        host.spawn_network(1, 1).unwrap();
        let state = |host: &World, tick, ack| {
            let s = host.snapshot(tick, vec![]);
            StatePacket {
                tick,
                elapsed_ms: 0,
                finished: false,
                ack,
                actors: s.actors,
                projectiles: vec![],
                explosions: vec![],
                removed: vec![],
            }
        };
        game.handle(
            &NetworkEvent::State {
                op: 1,
                epoch: 7,
                packet: state(&host, 6, 0),
            },
            &mut replica,
            &config,
        )
        .unwrap();
        for _ in 0..12 {
            game.advance(
                1. / 120.,
                &mut replica,
                &FlightInput::default(),
                0,
                Vec3::Z,
                true,
            );
        }
        let before = replica.render_body(1).position;
        host.actors[1].body.position.x += 5.;
        let mut expected = host.actors[1].body;
        for _ in 0..6 {
            expected.step(Controls::default(), config.flight, config.rules.fixed_dt());
        }
        game.handle(
            &NetworkEvent::State {
                op: 1,
                epoch: 7,
                packet: state(&host, 12, 6),
            },
            &mut replica,
            &config,
        )
        .unwrap();
        assert!(replica.actors[1].body.position.distance(expected.position) < 0.02);
        assert!(
            replica.render_body(1).position.distance(before) < 0.001,
            "correction must preserve visual continuity"
        );
        let body = replica.actors[1].body.position;
        game.handle(
            &NetworkEvent::State {
                op: 1,
                epoch: 6,
                packet: state(&host, 999, 6),
            },
            &mut replica,
            &config,
        )
        .unwrap();
        assert_eq!(replica.actors[1].body.position, body);
        host.destroy(1, None);
        host.actors[1].respawn = 0.;
        host.spawn_network(1, 1).unwrap();
        game.handle(
            &NetworkEvent::State {
                op: 1,
                epoch: 7,
                packet: state(&host, 18, 6),
            },
            &mut replica,
            &config,
        )
        .unwrap();
        assert!(game.game.as_ref().unwrap().predicted.is_empty());
        assert_eq!(replica.visual_offset, Vec3::ZERO);
    }
}
