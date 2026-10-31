//! Small screen controller and retained display list. No game/network state lives in widgets.

use crate::game::net_types::GameAction;
use crate::networking::{
    HostOptions, LinkScope, NetworkEvent, PeerInfo, Request as NetworkRequest, ServerListing,
};
use std::{collections::VecDeque, time::Duration};

pub const DESIGN_SIZE: [f32; 2] = [1280., 720.];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Splash,
    Start,
    Settings,
    ServerBrowser,
    Joining,
    Host,
    Lobby,
    Spawn,
    Gameplay,
    Pause,
    Report,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Pause,
    Back,
    Backspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiImage {
    Splash,
    Menu,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TargetMarker {
    pub position: [i32; 2],
    pub number: usize,
    pub distance: u32,
    pub health: u8,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HudData {
    pub speed: u32,
    pub altitude: u32,
    pub health: u32,
    pub weapon: &'static str,
    pub ammo: u32,
    pub missiles: u32,
    pub throttle: u32,
    pub radar_progress: u32,
    pub radar_locked: bool,
    pub incoming_lock: bool,
    pub incoming_missile: bool,
    pub kills: u32,
    pub deaths: u32,
    pub shots: u32,
    pub hits: u32,
    pub seconds: u32,
    pub outside: bool,
    pub aim: Option<[i32; 2]>,
    pub sight: Option<[i32; 2]>,
    pub targets: Vec<TargetMarker>,
}
impl Default for HudData {
    fn default() -> Self {
        Self {
            speed: 0,
            altitude: 0,
            health: 100,
            weapon: "GUNS",
            ammo: 800,
            missiles: 8,
            throttle: 55,
            radar_progress: 0,
            radar_locked: false,
            incoming_lock: false,
            incoming_missile: false,
            kills: 0,
            deaths: 0,
            shots: 0,
            hits: 0,
            seconds: 180,
            outside: false,
            aim: None,
            sight: Some([640, 360]),
            targets: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    fn contains(self, point: [f32; 2]) -> bool {
        point[0] >= self.x
            && point[0] < self.x + self.w
            && point[1] >= self.y
            && point[1] < self.y + self.h
    }
}

#[derive(Clone, Debug)]
pub enum DrawCommand {
    Rect(Rect, [f32; 4]),
    Text {
        position: [f32; 2],
        text: String,
        scale: f32,
        color: [f32; 4],
    },
    Image(UiImage, Rect, [f32; 4]),
}

#[derive(Default)]
pub struct UiFrame {
    pub commands: Vec<DrawCommand>,
    pub revision: u64,
    pub show_world: bool,
    pub world_region: Option<Rect>,
}

impl UiFrame {
    /// Optional 3D preview region, transformed with the same canvas as UI hit testing.
    pub fn world_viewport(&self, viewport: [u32; 2]) -> Option<[f32; 4]> {
        let region = self.world_region?;
        let scale = (viewport[0] as f32 / DESIGN_SIZE[0]).min(viewport[1] as f32 / DESIGN_SIZE[1]);
        Some([
            (viewport[0] as f32 - DESIGN_SIZE[0] * scale) * 0.5 + region.x * scale,
            (viewport[1] as f32 - DESIGN_SIZE[1] * scale) * 0.5 + region.y * scale,
            region.w * scale,
            region.h * scale,
        ])
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub volume: u8,
    pub music: u8,
    pub control_scheme: usize,
    pub vsync: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 75,
            music: 50,
            control_scheme: 3,
            vsync: false,
        }
    }
}

const CONTROL_SCHEMES: [&str; 4] = [
    "KEYBOARD / REALISTIC",
    "KEYBOARD / SIMPLE",
    "MOUSE / REALISTIC",
    "MOUSE / SIMPLE",
];

/// Future networking should deliver these events to the controller on the main thread.
pub enum SessionEvent {
    Connected { match_in_progress: bool },
    MatchStarted,
    AircraftDestroyed,
    MatchEnded,
    ConnectionLost(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Continue,
    Play,
    Settings,
    Quit,
    Back,
    Reset,
    Volume,
    Music,
    Controls,
    Vsync,
    Server(usize),
    Refresh,
    PreviousPage,
    NextPage,
    Join,
    JoinAddress,
    ConnectAddress,
    PasteAddress,
    CopyLink,
    Host,
    HostName,
    Mode,
    Map,
    MaxPlayers,
    Create,
    Ready,
    StartMatch,
    Team,
    Aircraft,
    Spawn,
    Resume,
    Respawn,
    EndMatch,
    Disconnect,
    Leave,
    ReportContinue,
}

#[derive(Clone)]
struct Widget {
    rect: Rect,
    label: String,
    action: Action,
    enabled: bool,
}

impl Widget {
    fn label_scale(&self) -> f32 {
        2_f32.min((self.rect.w - 40.) / (self.label.len().max(1) as f32 * 8.))
    }

    fn left_chevron_hit(&self, point: [f32; 2]) -> bool {
        self.label.find('<').is_some_and(|index| {
            let x = self.rect.x + 20. + index as f32 * 8. * self.label_scale();
            point[0] >= x - 8. && point[0] < x + 8. * self.label_scale() + 8.
        })
    }
}

#[derive(Clone)]
struct Session {
    host: bool,
    name: String,
    ready: bool,
    map: usize,
    mode: usize,
    max_players: usize,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            host: false,
            name: "OFFLINE SESSION".into(),
            ready: false,
            map: 0,
            mode: 0,
            max_players: 8,
        }
    }
}

const MAPS: [&str; 2] = ["WORLD 01", "OPEN SKY (PLACEHOLDER)"];
const MODES: [&str; 2] = ["TEAM DOGFIGHT", "FREE FOR ALL"];

const SERVERS: [(&str, &str, bool); 3] = [
    ("VANGUARD TRAINING", "LOBBY      1 / 8       DEMO", false),
    ("CANYON DOGFIGHT", "IN MATCH   3 / 8       DEMO", true),
    ("AFTERBURNER", "LOBBY      2 / 16      DEMO", false),
];

pub struct UiController {
    screen: Screen,
    settings_return: Screen,
    pub settings: Settings,
    session: Option<Session>,
    focused: usize,
    pressed: Option<usize>,
    pointer: Option<[f32; 2]>,
    editing: bool,
    dirty: bool,
    frame: UiFrame,
    widgets: Vec<Widget>,
    timer: f32,
    selected_server: usize,
    host_name: String,
    host_mode: usize,
    host_map: usize,
    host_max: usize,
    team: usize,
    hud: HudData,
    message: String,
    pub quit_requested: bool,
    network_enabled: bool,
    online_game: bool,
    game_epoch: u32,
    action_counter: u32,
    network_operation: u64,
    network_requests: VecDeque<NetworkRequest>,
    server_list: Vec<ServerListing>,
    server_page: usize,
    network_roster: Vec<PeerInfo>,
    local_player: u64,
    network_endpoint: String,
    join_address: String,
    share_link: String,
    share_scope: LinkScope,
    edit_target: Action,
    copy_text: Option<String>,
    paste_requested: bool,
}

impl Default for UiController {
    fn default() -> Self {
        Self {
            screen: Screen::Splash,
            settings_return: Screen::Start,
            settings: Settings::default(),
            session: None,
            focused: 0,
            pressed: None,
            pointer: None,
            editing: false,
            dirty: true,
            frame: UiFrame::default(),
            widgets: Vec::new(),
            timer: 0.,
            selected_server: 0,
            host_name: "LOCAL DOGFIGHT".into(),
            host_mode: 0,
            host_map: 0,
            host_max: 2,
            team: 0,
            hud: HudData::default(),
            message: String::new(),
            quit_requested: false,
            network_enabled: false,
            online_game: false,
            game_epoch: 0,
            action_counter: 0,
            network_operation: 0,
            network_requests: VecDeque::new(),
            server_list: Vec::new(),
            server_page: 0,
            network_roster: Vec::new(),
            local_player: 0,
            network_endpoint: String::new(),
            join_address: String::new(),
            share_link: String::new(),
            share_scope: LinkScope::Local,
            edit_target: Action::HostName,
            copy_text: None,
            paste_requested: false,
        }
    }
}

impl UiController {
    pub fn accepts_network_event(&self, event: &NetworkEvent) -> bool {
        event
            .operation()
            .is_none_or(|op| op == self.network_operation)
    }
    pub fn shows_world(&self) -> bool {
        matches!(
            self.screen,
            Screen::Gameplay | Screen::Pause | Screen::Spawn | Screen::Report
        ) || (self.screen == Screen::Settings && self.settings_return == Screen::Pause)
    }
    fn game_action(&mut self, action: GameAction) {
        self.action_counter = self.action_counter.saturating_add(1);
        self.network_requests.push_back(NetworkRequest::Action {
            op: self.network_operation,
            epoch: self.game_epoch,
            action_id: self.action_counter,
            action,
            team: self.team as u8,
        });
    }
    pub fn enable_networking(&mut self) {
        self.network_enabled = true;
        self.dirty = true;
    }
    pub fn take_network_requests(&mut self) -> VecDeque<NetworkRequest> {
        std::mem::take(&mut self.network_requests)
    }
    pub fn request_paste(&mut self) {
        if self.network_enabled && (self.screen == Screen::ServerBrowser || self.editing) {
            self.paste_requested = true;
        }
    }
    pub fn take_paste_request(&mut self) -> bool {
        std::mem::take(&mut self.paste_requested)
    }
    pub fn take_copy_text(&mut self) -> Option<String> {
        self.copy_text.take()
    }
    pub fn copy_invitation(&mut self) {
        if !self.share_link.is_empty() && self.screen == Screen::Lobby {
            self.copy_text = Some(self.share_link.clone());
        }
    }
    pub fn clipboard_result(&mut self, message: &str) {
        self.message = message.into();
        self.dirty = true;
    }
    pub fn paste_text(&mut self, text: &str) {
        if self.screen == Screen::ServerBrowser {
            self.join_address = text
                .trim()
                .chars()
                .filter(|c| c.is_ascii_graphic())
                .take(4096)
                .collect();
        } else if self.editing {
            match self.edit_target {
                Action::HostName => self.host_name.clear(),
                _ => return,
            }
            self.text(text.trim());
        }
        self.dirty = true;
    }
    fn operation(&mut self) -> u64 {
        self.network_operation = self.network_operation.wrapping_add(1);
        self.network_operation
    }
    fn discover(&mut self) {
        let op = self.operation();
        self.message = "SEARCHING LAN...".into();
        self.network_requests
            .push_back(NetworkRequest::Discover { op });
        self.dirty = true;
    }
    pub fn connect_direct(&mut self, endpoint: String) {
        self.network_enabled = true;
        let op = self.operation();
        self.session = Some(Session {
            name: "CONNECTING".into(),
            ..Default::default()
        });
        self.network_requests
            .push_back(NetworkRequest::Join { op, endpoint });
        self.go(Screen::Joining);
    }
    pub fn network_request_failed(&mut self, message: String) {
        self.network_event(NetworkEvent::Error {
            op: self.network_operation,
            message,
        });
    }
    pub fn network_event(&mut self, event: NetworkEvent) {
        match event {
            NetworkEvent::GameStarted { op, setup, .. }
                if op == self.network_operation && self.session.is_some() =>
            {
                self.online_game = true;
                self.game_epoch = setup.epoch;
                self.action_counter = 0;
                self.message.clear();
                self.session_event(SessionEvent::MatchStarted);
            }
            NetworkEvent::ReturnedLobby { op } if op == self.network_operation => {
                self.online_game = false;
                self.message.clear();
                self.go(Screen::Lobby);
            }
            NetworkEvent::ActionResult {
                op,
                epoch,
                action_id,
                accepted,
                reason,
                ..
            } if op == self.network_operation
                && epoch == self.game_epoch
                && action_id == self.action_counter =>
            {
                if !accepted {
                    self.message = reason.to_uppercase();
                    if self.screen == Screen::Gameplay {
                        self.go(Screen::Spawn);
                    }
                    self.dirty = true;
                }
            }
            NetworkEvent::Servers { op, servers } if op == self.network_operation => {
                self.server_list = servers;
                self.selected_server = self
                    .selected_server
                    .min(self.server_list.len().saturating_sub(1));
                self.server_page = 0;
                self.message = if self.server_list.is_empty() {
                    "NO MATCHES FOUND".into()
                } else {
                    String::new()
                };
                self.dirty = true;
            }
            NetworkEvent::Connected { op, info, endpoint }
                if op == self.network_operation && self.screen == Screen::Joining =>
            {
                self.local_player = info.player_id;
                self.network_roster = info.players;
                self.network_endpoint = endpoint;
                self.session = Some(Session {
                    host: info.host,
                    name: info.name,
                    ready: self
                        .network_roster
                        .iter()
                        .any(|p| p.id == info.player_id && p.ready),
                    map: MAPS.iter().position(|name| *name == info.map).unwrap_or(0),
                    mode: MODES
                        .iter()
                        .position(|name| *name == info.mode)
                        .unwrap_or(0),
                    max_players: info.max_players,
                });
                self.message.clear();
                self.go(Screen::Lobby);
            }
            NetworkEvent::Lobby { op, players } if op == self.network_operation => {
                if let Some(session) = &mut self.session {
                    session.ready = players.iter().any(|p| p.id == self.local_player && p.ready);
                }
                self.network_roster = players;
                self.dirty = true;
            }
            NetworkEvent::ShareLink {
                op,
                link,
                scope,
                warning,
            } if op == self.network_operation && self.screen == Screen::Lobby => {
                self.share_link = link;
                self.share_scope = scope;
                if !warning.is_empty() {
                    self.message = warning.to_uppercase();
                }
                self.dirty = true;
            }
            NetworkEvent::Error { op, message } if op == self.network_operation => {
                if self.screen == Screen::Joining {
                    self.leave();
                }
                self.message = message.to_uppercase();
                self.dirty = true;
            }
            NetworkEvent::Disconnected { op, reason } if op == self.network_operation => {
                self.session_event(SessionEvent::ConnectionLost(reason))
            }
            NetworkEvent::WorkerStopped { reason } => {
                if self.session.is_some() {
                    self.session_event(SessionEvent::ConnectionLost(reason));
                } else {
                    self.message = reason.to_uppercase();
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    fn browser_rows(&self) -> Vec<(usize, String, String)> {
        if self.network_enabled {
            self.server_list
                .iter()
                .enumerate()
                .skip(self.server_page * 3)
                .take(3)
                .map(|(i, s)| {
                    (
                        i,
                        s.name.chars().take(26).collect(),
                        format!(
                            "{:<10} {:2} / {:2}   {}",
                            if s.in_progress { "IN MATCH" } else { "LOBBY" },
                            s.players,
                            s.max_players,
                            if s.reachability == "LAN" {
                                "LAN"
                            } else {
                                "UNVERIFIED"
                            }
                        ),
                    )
                })
                .collect()
        } else {
            SERVERS
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.0.into(), s.1.into()))
                .collect()
        }
    }
    pub fn screen(&self) -> Screen {
        self.screen
    }
    pub fn needs_redraw(&self) -> bool {
        self.dirty
    }
    pub fn editing_text(&self) -> bool {
        self.editing
    }
    pub fn is_gameplay(&self) -> bool {
        self.screen == Screen::Gameplay
    }

    pub fn team(&self) -> usize {
        self.team
    }
    pub fn set_hud(&mut self, hud: HudData) {
        if self.hud != hud {
            self.hud = hud;
            self.dirty = true;
        }
    }

    /// Direct entry to the existing 3D preview for development and smoke tests.
    pub fn start_preview(&mut self) {
        self.session = Some(Session {
            host: true,
            name: "LOCAL PREVIEW".into(),
            ready: true,
            ..Default::default()
        });
        self.go(Screen::Gameplay);
    }

    pub fn frame(&mut self) -> &UiFrame {
        self.ensure_layout();
        &self.frame
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.focused = 0;
        self.pressed = None;
        self.editing = false;
        self.timer = 0.;
        self.dirty = true;
    }

    pub fn tick(&mut self, delta: Duration) {
        let previous = self.timer;
        self.timer += delta.as_secs_f32();
        match self.screen {
            Screen::Splash if self.timer >= 2.0 => self.go(Screen::Start),
            Screen::Joining if !self.network_enabled && self.timer >= 1.2 => {
                self.session_event(SessionEvent::Connected {
                    match_in_progress: SERVERS[self.selected_server].2,
                })
            }
            Screen::Joining | Screen::Splash
                if (previous * 20.) as u32 != (self.timer * 20.) as u32 =>
            {
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn session_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::ConnectionLost(reason) => {
                self.online_game = false;
                if self.network_enabled {
                    self.operation();
                    self.network_requests.push_back(NetworkRequest::Leave);
                    self.network_roster.clear();
                    self.share_link.clear();
                }
                self.session = None;
                self.message = format!("CONNECTION LOST: {}", reason.to_uppercase());
                self.go(Screen::ServerBrowser);
            }
            SessionEvent::Connected { match_in_progress } if self.screen == Screen::Joining => {
                self.go(if match_in_progress {
                    Screen::Spawn
                } else {
                    Screen::Lobby
                });
            }
            SessionEvent::MatchStarted if self.session.is_some() => self.go(Screen::Spawn),
            SessionEvent::AircraftDestroyed if self.session.is_some() => self.go(Screen::Spawn),
            SessionEvent::MatchEnded if self.session.is_some() => self.go(Screen::Report),
            _ => {}
        }
    }

    /// Returns true when the UI consumes the key; future flight input should respect this.
    pub fn key(&mut self, key: UiKey) -> bool {
        self.ensure_layout();
        if key == UiKey::Pause {
            match self.screen {
                Screen::Gameplay => self.go(Screen::Pause),
                Screen::Pause => self.go(Screen::Gameplay),
                Screen::Settings if self.settings_return == Screen::Pause => self.go(Screen::Pause),
                _ => {}
            }
            return true;
        }
        if self.screen == Screen::Gameplay {
            return false;
        }
        match key {
            UiKey::Up | UiKey::Down => {
                self.editing = false;
                let count = self.widgets.len();
                if count > 0 {
                    let step = if key == UiKey::Up { count - 1 } else { 1 };
                    for _ in 0..count {
                        self.focused = (self.focused + step) % count;
                        if self.widgets[self.focused].enabled {
                            break;
                        }
                    }
                    self.dirty = true;
                }
            }
            UiKey::Left | UiKey::Right => {
                if let Some(widget) = self.widgets.get(self.focused).cloned() {
                    let delta = if key == UiKey::Left { -1 } else { 1 };
                    if !self.adjust(widget.action, delta) {
                        return self.key(if delta < 0 { UiKey::Up } else { UiKey::Down });
                    }
                }
            }
            UiKey::Enter => {
                if let Some(widget) = self.widgets.get(self.focused).cloned() {
                    if widget.enabled {
                        self.activate(widget.action);
                    }
                }
            }
            UiKey::Back => self.back(),
            UiKey::Backspace if self.editing => {
                match self.edit_target {
                    Action::HostName => {
                        self.host_name.pop();
                    }
                    Action::JoinAddress => {
                        self.join_address.pop();
                    }
                    _ => {}
                }
                self.dirty = true;
            }
            _ => {}
        }
        true
    }

    pub fn text(&mut self, text: &str) {
        if self.editing {
            let (buffer, limit, uppercase) = match self.edit_target {
                Action::HostName => (&mut self.host_name, 24, true),
                Action::JoinAddress => (&mut self.join_address, 4096, false),
                _ => return,
            };
            for c in text.chars().filter(|c| c.is_ascii_graphic() || *c == ' ') {
                if buffer.len() >= limit {
                    break;
                }
                buffer.push(if uppercase { c.to_ascii_uppercase() } else { c });
            }
            self.dirty = true;
        }
    }

    pub fn pointer_move(&mut self, physical: [f32; 2], viewport: [u32; 2]) {
        self.ensure_layout();
        self.pointer = to_design(physical, viewport);
        if let Some(point) = self.pointer {
            if let Some(index) = self
                .widgets
                .iter()
                .position(|w| w.enabled && w.rect.contains(point))
            {
                if index != self.focused {
                    self.focused = index;
                    self.editing = false;
                    self.dirty = true;
                }
            }
        }
    }

    pub fn pointer_button(&mut self, down: bool) {
        self.ensure_layout();
        let hit = self.pointer.and_then(|p| {
            self.widgets
                .iter()
                .position(|w| w.enabled && w.rect.contains(p))
        });
        if down {
            self.pressed = hit;
            if let Some(index) = hit {
                self.focused = index;
                self.dirty = true;
            }
        } else {
            let pressed = self.pressed.take();
            if let Some(index) = pressed.filter(|_| pressed == hit) {
                let widget = self.widgets[index].clone();
                let decrease = self.pointer.is_some_and(|p| widget.left_chevron_hit(p));
                if !decrease || !self.adjust(widget.action, -1) {
                    self.activate(widget.action);
                }
            }
            self.dirty = true;
        }
    }

    pub fn cancel_pointer(&mut self) {
        self.pressed = None;
        self.pointer = None;
        self.dirty = true;
    }

    fn back(&mut self) {
        if self.editing {
            self.editing = false;
            self.dirty = true;
            return;
        }
        match self.screen {
            Screen::Settings => self.go(self.settings_return),
            Screen::Start => {}
            Screen::ServerBrowser => self.go(Screen::Start),
            Screen::Host | Screen::Joining | Screen::Lobby => self.leave(),
            Screen::Pause => self.go(Screen::Gameplay),
            Screen::Spawn => self.go(Screen::Lobby),
            Screen::Report => self.activate(Action::ReportContinue),
            _ => self.go(Screen::Start),
        }
    }

    fn leave(&mut self) {
        self.online_game = false;
        if self.network_enabled {
            self.operation();
            self.network_requests.push_back(NetworkRequest::Leave);
            self.network_roster.clear();
            self.share_link.clear();
        }
        self.session = None;
        self.message = if self.network_enabled {
            String::new()
        } else {
            "OFFLINE PREVIEW / NO NETWORK CONNECTION".into()
        };
        self.go(Screen::ServerBrowser);
    }

    fn adjust(&mut self, action: Action, delta: i32) -> bool {
        match action {
            Action::Volume => {
                self.settings.volume = (self.settings.volume as i32 + delta * 5).clamp(0, 100) as u8
            }
            Action::Music => {
                self.settings.music = (self.settings.music as i32 + delta * 5).clamp(0, 100) as u8
            }
            Action::Controls => {
                self.settings.control_scheme = cycle(self.settings.control_scheme, delta, 4)
            }
            Action::Vsync => self.settings.vsync = !self.settings.vsync,
            Action::Mode => self.host_mode = cycle(self.host_mode, delta, 2),
            Action::Map => self.host_map = cycle(self.host_map, delta, 2),
            Action::MaxPlayers => self.host_max = cycle(self.host_max, delta, 4),
            Action::Team => self.team = cycle(self.team, delta, 2),
            Action::Aircraft => {
                self.message = "F-16 IS THE ONLY AVAILABLE AIRCRAFT".into();
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    fn activate(&mut self, action: Action) {
        if self.adjust(action, 1) {
            return;
        }
        match action {
            Action::Continue => self.go(Screen::Start),
            Action::Play => {
                if self.network_enabled {
                    self.discover();
                } else {
                    self.message = "OFFLINE PREVIEW / SIMULATED SERVERS".into();
                }
                self.go(Screen::ServerBrowser);
            }
            Action::Settings => {
                self.settings_return = self.screen;
                self.go(Screen::Settings);
            }
            Action::Quit => self.quit_requested = true,
            Action::Back => self.back(),
            Action::Reset => {
                self.settings = Settings::default();
                self.dirty = true;
            }
            Action::Server(index) => {
                self.selected_server = index;
                self.dirty = true;
            }
            Action::Refresh => {
                if self.network_enabled {
                    self.discover();
                } else {
                    self.message = "DEMO LIST REFRESHED / REAL DISCOVERY NOT IMPLEMENTED".into();
                }
                self.dirty = true;
            }
            Action::PreviousPage => {
                self.server_page = self.server_page.saturating_sub(1);
                self.dirty = true;
            }
            Action::NextPage => {
                if (self.server_page + 1) * 3 < self.server_list.len() {
                    self.server_page += 1;
                }
                self.dirty = true;
            }
            Action::Join => {
                if self.network_enabled {
                    if let Some(server) = self.server_list.get(self.selected_server) {
                        self.connect_direct(server.endpoint.clone());
                    }
                    return;
                }
                self.session = Some(Session {
                    host: false,
                    name: SERVERS[self.selected_server].0.into(),
                    ready: false,
                    max_players: if self.selected_server == 2 { 16 } else { 8 },
                    ..Default::default()
                });
                self.go(Screen::Joining);
            }
            Action::Host => self.go(Screen::Host),
            Action::HostName | Action::JoinAddress => {
                self.edit_target = action;
                self.editing = !self.editing;
                self.dirty = true;
            }
            Action::ConnectAddress => {
                if !self.join_address.trim().is_empty() {
                    self.connect_direct(self.join_address.trim().into());
                }
            }
            Action::PasteAddress => self.request_paste(),
            Action::CopyLink => self.copy_invitation(),
            Action::Create => {
                let name = if self.host_name.trim().is_empty() {
                    "LOCAL DOGFIGHT"
                } else {
                    self.host_name.trim()
                };
                self.session = Some(Session {
                    host: true,
                    name: name.into(),
                    ready: true,
                    map: self.host_map,
                    mode: self.host_mode,
                    max_players: [2, 4, 8, 16][self.host_max],
                });
                if self.network_enabled {
                    let options = HostOptions {
                        name: name.into(),
                        max_players: [2, 4, 8, 16][self.host_max],
                        map: MAPS[self.host_map].into(),
                        mode: MODES[self.host_mode].into(),
                    };
                    let op = self.operation();
                    self.network_requests
                        .push_back(NetworkRequest::Host { op, options });
                    self.go(Screen::Joining);
                } else {
                    self.go(Screen::Lobby);
                }
            }
            Action::Ready => {
                if let Some(session) = &mut self.session {
                    session.ready = !session.ready;
                    if self.network_enabled {
                        self.network_requests.push_back(NetworkRequest::Ready {
                            op: self.network_operation,
                            ready: session.ready,
                        });
                    }
                }
                self.dirty = true;
            }
            Action::StartMatch => {
                if self.network_enabled {
                    self.network_requests.push_back(NetworkRequest::Start {
                        op: self.network_operation,
                    });
                } else {
                    self.session_event(SessionEvent::MatchStarted);
                }
            }
            Action::Spawn => {
                if self.online_game {
                    self.game_action(GameAction::Spawn);
                }
                self.go(Screen::Gameplay);
            }
            Action::Resume => self.go(Screen::Gameplay),
            Action::Respawn => {
                if self.online_game {
                    self.game_action(GameAction::Respawn);
                }
                self.session_event(SessionEvent::AircraftDestroyed);
            }
            Action::EndMatch => {
                if self.online_game {
                    self.game_action(GameAction::End);
                } else {
                    self.session_event(SessionEvent::MatchEnded);
                }
            }
            Action::Disconnect => {
                self.session_event(SessionEvent::ConnectionLost("SIMULATED LINK LOSS".into()))
            }
            Action::Leave => self.leave(),
            Action::ReportContinue => {
                if self.online_game {
                    if self.session.as_ref().is_some_and(|s| s.host) {
                        self.network_requests
                            .push_back(NetworkRequest::ReturnLobby {
                                op: self.network_operation,
                                epoch: self.game_epoch,
                            });
                    }
                    self.go(Screen::Lobby);
                    return;
                }
                self.go(if self.session.as_ref().is_some_and(|s| s.host) {
                    Screen::Host
                } else {
                    Screen::Lobby
                });
            }
            _ => {}
        }
    }

    fn ensure_layout(&mut self) {
        if !self.dirty {
            return;
        }
        let mut paint = Painter::default();
        let world = matches!(
            self.screen,
            Screen::Gameplay | Screen::Pause | Screen::Spawn | Screen::Report
        ) || (self.screen == Screen::Settings && self.settings_return == Screen::Pause);
        paint.background(
            if self.screen == Screen::Splash {
                UiImage::Splash
            } else {
                UiImage::Menu
            },
            world,
        );
        let session_name = self
            .session
            .as_ref()
            .map_or("OFFLINE SESSION", |s| s.name.as_str());
        match self.screen {
            Screen::Splash => {
                paint.rect(Rect::new(0., 0., 1280., 720.), [0.01, 0.025, 0.05, 0.35]);
                paint.text([64., 430.], "JETGAME", 7., WHITE);
                paint.text([68., 500.], "MULTIPLAYER AIR COMBAT", 2., CYAN);
                paint.button(
                    Rect::new(68., 570., 400., 52.),
                    "ENTER / CONTINUE",
                    Action::Continue,
                );
                paint.text([68., 645.], "ENGINE PREVIEW  /  OFFLINE", 1.5, MUTED);
            }
            Screen::Start => {
                paint.text([64., 110.], "JETGAME", 7., WHITE);
                paint.text([68., 184.], "TAKE TO THE SKIES", 2., CYAN);
                for (i, (label, action)) in [
                    ("PLAY", Action::Play),
                    ("SETTINGS", Action::Settings),
                    ("QUIT", Action::Quit),
                ]
                .into_iter()
                .enumerate()
                {
                    paint.button(
                        Rect::new(68., 270. + i as f32 * 74., 360., 56.),
                        label,
                        action,
                    );
                }
                paint.panel(Rect::new(770., 350., 440., 215.));
                paint.text([796., 378.], "FLIGHT SYSTEMS ONLINE", 2., CYAN);
                paint.text([796., 424.], "AIRCRAFT SYSTEMS READY", 1.5, WHITE);
                paint.text(
                    [796., 458.],
                    if self.network_enabled {
                        "HOST OR FIND A MATCH"
                    } else {
                        "NETWORK: OFFLINE PREVIEW"
                    },
                    1.5,
                    MUTED,
                );
                paint.text([796., 492.], "ALL MENUS AVAILABLE", 1.5, MUTED);
            }
            Screen::Settings => {
                paint.header("SETTINGS", "LEFT / RIGHT TO ADJUST  -  ENTER TO CYCLE");
                paint.panel(Rect::new(260., 150., 760., 466.));
                let labels = [
                    format!("MASTER VOLUME     < {:3}% >", self.settings.volume),
                    format!("MUSIC VOLUME      < {:3}% >", self.settings.music),
                    format!(
                        "CONTROLS   < {} >",
                        CONTROL_SCHEMES[self.settings.control_scheme]
                    ),
                    format!(
                        "VSYNC             < {} >",
                        if self.settings.vsync { "ON" } else { "OFF" }
                    ),
                    "RESET TO DEFAULTS".into(),
                    "BACK / KEEP SETTINGS".into(),
                ];
                for (i, (label, action)) in labels
                    .iter()
                    .zip([
                        Action::Volume,
                        Action::Music,
                        Action::Controls,
                        Action::Vsync,
                        Action::Reset,
                        Action::Back,
                    ])
                    .enumerate()
                {
                    paint.button(
                        Rect::new(290., 178. + i as f32 * 66., 700., 48.),
                        label,
                        action,
                    );
                }
                paint.text(
                    [280., 630.],
                    "SETTINGS ARE KEPT FOR THIS SESSION",
                    1.3,
                    MUTED,
                );
            }
            Screen::ServerBrowser => {
                paint.header(
                    "SERVER BROWSER",
                    if self.network_enabled {
                        "LAN MATCHES / OR JOIN WITH AN INVITATION"
                    } else {
                        "OFFLINE PREVIEW  /  SELECT A SERVER, THEN JOIN"
                    },
                );
                paint.panel(Rect::new(64., 154., 1152., 320.));
                paint.text([90., 179.], "SERVER", 1.5, CYAN);
                paint.text([578., 179.], "STATUS     PLAYERS     NETWORK", 1.5, CYAN);
                for (row, (i, name, _)) in self.browser_rows().iter().enumerate() {
                    let y = 218. + row as f32 * 76.;
                    if *i == self.selected_server {
                        paint.rect(Rect::new(68., y, 4., 58.), CYAN);
                    }
                    paint.button(Rect::new(84., y, 1112., 58.), name, Action::Server(*i));
                }
                if self.network_enabled && self.server_list.is_empty() {
                    paint.text([94., 270.], "NO MATCHES FOUND", 2., MUTED);
                }
                if self.network_enabled && self.server_list.len() > 3 {
                    paint.button(Rect::new(1010., 170., 80., 30.), "<", Action::PreviousPage);
                    paint.button(Rect::new(1110., 170., 80., 30.), ">", Action::NextPage);
                }
                if self.network_enabled {
                    let tail: String = self
                        .join_address
                        .chars()
                        .rev()
                        .take(65)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    paint.button(
                        Rect::new(64., 454., 756., 44.),
                        &format!(
                            "ADDRESS: {tail}{}",
                            if self.editing && self.edit_target == Action::JoinAddress {
                                "_"
                            } else {
                                ""
                            }
                        ),
                        Action::JoinAddress,
                    );
                    paint.button(
                        Rect::new(838., 454., 174., 44.),
                        "PASTE",
                        Action::PasteAddress,
                    );
                    paint.enabled_button(
                        Rect::new(1030., 454., 186., 44.),
                        "CONNECT",
                        Action::ConnectAddress,
                        !self.join_address.trim().is_empty(),
                    );
                }
                for (i, (label, action)) in [
                    ("REFRESH", Action::Refresh),
                    ("JOIN SELECTED", Action::Join),
                    ("HOST MATCH", Action::Host),
                    ("BACK", Action::Back),
                ]
                .into_iter()
                .enumerate()
                {
                    paint.enabled_button(
                        Rect::new(64. + i as f32 * 294., 516., 270., 54.),
                        label,
                        action,
                        action != Action::Join
                            || !self.network_enabled
                            || !self.server_list.is_empty(),
                    );
                }
                paint.notice(&self.message);
            }
            Screen::Joining => {
                paint.header(
                    "JOINING MATCH",
                    if self.network_enabled {
                        "CONNECTING TO SERVER"
                    } else {
                        "OFFLINE PREVIEW / CONNECTING TO A SIMULATED SESSION"
                    },
                );
                paint.panel(Rect::new(260., 230., 760., 240.));
                paint.text([294., 263.], session_name, 2., WHITE);
                paint.text([294., 310.], "ESTABLISHING SESSION...", 1.5, MUTED);
                paint.rect(Rect::new(294., 352., 690., 8.), BORDER);
                paint.rect(
                    if self.network_enabled {
                        Rect::new(294. + 600. * (self.timer * 0.4).fract(), 352., 90., 8.)
                    } else {
                        Rect::new(294., 352., 690. * (self.timer / 1.2).min(1.), 8.)
                    },
                    CYAN,
                );
                paint.button(Rect::new(448., 395., 384., 48.), "CANCEL", Action::Leave);
            }
            Screen::Host => {
                paint.header(
                    "HOST MATCH",
                    if self.network_enabled {
                        "CONFIGURE YOUR SERVER"
                    } else {
                        "CONFIGURE AN OFFLINE PREVIEW SESSION"
                    },
                );
                paint.panel(Rect::new(260., 150., 760., 470.));
                let cursor = if self.editing && self.edit_target == Action::HostName {
                    "_"
                } else {
                    ""
                };
                let labels = [
                    format!("NAME: {}{}", self.host_name, cursor),
                    format!("MODE: < {} >", MODES[self.host_mode]),
                    format!("MAP: < {} >", MAPS[self.host_map]),
                    format!("PLAYER LIMIT: < {} >", [2, 4, 8, 16][self.host_max]),
                    if self.network_enabled {
                        "CREATE LOBBY".into()
                    } else {
                        "CREATE PREVIEW LOBBY".into()
                    },
                    "CANCEL".into(),
                ];
                let actions = [
                    Action::HostName,
                    Action::Mode,
                    Action::Map,
                    Action::MaxPlayers,
                    Action::Create,
                    Action::Back,
                ];
                for (i, (label, action)) in labels.iter().zip(actions).enumerate() {
                    paint.button(
                        Rect::new(290., 178. + i as f32 * 66., 700., 48.),
                        label,
                        action,
                    );
                }
                paint.text(
                    [290., 634.],
                    "ENTER TO EDIT / CTRL+V TO PASTE / ENTER TO FINISH",
                    1.2,
                    MUTED,
                );
            }
            Screen::Lobby => {
                paint.header("WAITING LOBBY", session_name);
                paint.panel(Rect::new(64., 160., 1152., 340.));
                paint.text([94., 188.], "PILOT ROSTER", 2., CYAN);
                let ready = self.session.as_ref().is_some_and(|s| s.ready);
                if self.network_enabled {
                    for (i, player) in self.network_roster.iter().take(8).enumerate() {
                        let name: String = player.name.chars().take(20).collect();
                        let prefix = if player.id == self.local_player {
                            "YOU / "
                        } else {
                            ""
                        };
                        paint.text(
                            [94., 236. + i as f32 * 28.],
                            &format!("{prefix}{name}"),
                            1.5,
                            WHITE,
                        );
                        paint.text(
                            [470., 236. + i as f32 * 28.],
                            if player.ready { "READY" } else { "NOT READY" },
                            1.5,
                            if player.ready { CYAN } else { MUTED },
                        );
                    }
                    if self.network_roster.len() > 8 {
                        paint.text(
                            [94., 470.],
                            &format!("+ {} MORE PILOTS", self.network_roster.len() - 8),
                            1.5,
                            MUTED,
                        );
                    }
                    paint.text([782., 244.], "CONNECTED", 2., CYAN);
                    let endpoint: String = self.network_endpoint.chars().take(36).collect();
                    paint.text([782., 294.], &endpoint, 1.2, MUTED);
                } else {
                    paint.text(
                        [94., 244.],
                        if ready {
                            "YOU                 READY"
                        } else {
                            "YOU                 NOT READY"
                        },
                        2.,
                        WHITE,
                    );
                    paint.text([94., 294.], "WINGMAN             PLACEHOLDER", 1.5, MUTED);
                    paint.text([94., 338.], "OPPONENT            PLACEHOLDER", 1.5, MUTED);
                    paint.text([782., 244.], "OFFLINE SESSION", 2., CYAN);
                    paint.text([782., 294.], "NO REAL PLAYERS CONNECTED", 1.5, MUTED);
                }
                if let Some(session) = &self.session {
                    paint.text(
                        [782., 338.],
                        &format!("MAP: {}", MAPS[session.map]),
                        1.5,
                        MUTED,
                    );
                    paint.text(
                        [782., 376.],
                        &format!("MODE: {}", MODES[session.mode]),
                        1.5,
                        MUTED,
                    );
                    paint.text(
                        [782., 414.],
                        &format!("PLAYER LIMIT: {}", session.max_players),
                        1.5,
                        MUTED,
                    );
                }
                let host = self.session.as_ref().is_some_and(|s| s.host);
                paint.enabled_button(
                    Rect::new(64., 538., 352., 54.),
                    if ready { "UNREADY" } else { "READY" },
                    Action::Ready,
                    !self.online_game,
                );
                paint.enabled_button(
                    Rect::new(464., 538., 352., 54.),
                    if self.network_enabled {
                        if host { "START MATCH" } else { "AWAITING HOST" }
                    } else if host {
                        "START MATCH PREVIEW"
                    } else {
                        "PREVIEW MATCH START"
                    },
                    Action::StartMatch,
                    ready && (!self.network_enabled || host) && !self.online_game,
                );
                paint.button(
                    Rect::new(864., 538., 352., 54.),
                    "LEAVE LOBBY",
                    Action::Leave,
                );
                if self.network_enabled {
                    if !self.share_link.is_empty() {
                        paint.text(
                            [782., 438.],
                            match self.share_scope {
                                LinkScope::Internet => "LINK: INTERNET ADDRESS",
                                LinkScope::Lan => "LINK: SAME NETWORK",
                                LinkScope::Local => "LINK: THIS COMPUTER ONLY",
                            },
                            1.1,
                            MUTED,
                        );
                        paint.button(
                            Rect::new(782., 458., 400., 34.),
                            "COPY JOIN LINK",
                            Action::CopyLink,
                        );
                    }
                    paint.notice(&self.message);
                }
            }
            Screen::Spawn => {
                paint.header("AIRCRAFT DEPLOYMENT", "SELECT YOUR TEAM");
                paint.panel(Rect::new(64., 160., 536., 414.));
                let labels = [
                    format!("TEAM: < {} >", ["BLUE", "ORANGE"][self.team]),
                    "AIRCRAFT: F-16".into(),
                    "SPAWN".into(),
                    "LEAVE MATCH".into(),
                ];
                for (i, (label, action)) in labels
                    .iter()
                    .zip([Action::Team, Action::Aircraft, Action::Spawn, Action::Leave])
                    .enumerate()
                {
                    paint.button(
                        Rect::new(88., 188. + i as f32 * 76., 488., 52.),
                        label,
                        action,
                    );
                }
                paint.notice(&self.message);
                paint.text([802., 190.], "AIRCRAFT PREVIEW", 2., CYAN);
                paint.text([802., 230.], "GUNS + MISSILES EQUIPPED", 1.5, MUTED);
                if self.message.starts_with("F-16") {
                    paint.notice(&self.message);
                }
            }
            Screen::Gameplay => {
                let hud = &self.hud;
                paint.panel(Rect::new(474., 54., 324., 38.));
                paint.panel(Rect::new(450., 668., 470., 38.));
                paint.text([32., 28.], "F-16", 2., WHITE);
                paint.text(
                    [940., 28.],
                    &format!(
                        "TIME {:02}:{:02}  KILLS {}/5",
                        hud.seconds / 60,
                        hud.seconds % 60,
                        hud.kills
                    ),
                    1.5,
                    WHITE,
                );
                paint.panel(Rect::new(24., 516., 340., 160.));
                paint.text(
                    [40., 532.],
                    &format!("HEALTH {:3}    THR {:3}%", hud.health, hud.throttle),
                    1.5,
                    WHITE,
                );
                paint.text(
                    [40., 564.],
                    &format!("SPD {:4} KM/H", hud.speed),
                    1.5,
                    WHITE,
                );
                paint.text(
                    [40., 596.],
                    &format!("ALT {:4} M", hud.altitude),
                    1.5,
                    WHITE,
                );
                paint.text(
                    [40., 628.],
                    &format!("{} {:4}   MSL {}", hud.weapon, hud.ammo, hud.missiles),
                    1.5,
                    CYAN,
                );
                if let Some([x, y]) = hud.sight {
                    for r in [
                        Rect::new(x as f32 - 15., y as f32, 10., 2.),
                        Rect::new(x as f32 + 7., y as f32, 10., 2.),
                        Rect::new(x as f32, y as f32 - 15., 2., 10.),
                        Rect::new(x as f32, y as f32 + 7., 2., 10.),
                    ] {
                        paint.rect(r, [0.6, 1., 0.9, 0.9]);
                    }
                }
                if let Some([x, y]) = hud.aim {
                    let x = x as f32;
                    let y = y as f32;
                    for r in [
                        Rect::new(x - 7., y - 7., 14., 1.),
                        Rect::new(x - 7., y + 7., 14., 1.),
                        Rect::new(x - 7., y - 7., 1., 14.),
                        Rect::new(x + 7., y - 7., 1., 14.),
                    ] {
                        paint.rect(r, CYAN);
                    }
                }
                for marker in &hud.targets {
                    let x = marker.position[0] as f32;
                    let y = marker.position[1] as f32;
                    let color = if marker.locked {
                        [1., 0.12, 0.04, 1.]
                    } else {
                        [1., 0.65, 0.1, 1.]
                    };
                    paint.rect(Rect::new(x - 24., y - 24., 48., 48.), [0., 0., 0., 0.20]);
                    paint.rect(
                        Rect::new(x - 28., y + 26., 102., 34.),
                        [0.005, 0.01, 0.02, 0.85],
                    );
                    for r in [
                        Rect::new(x - 22., y - 22., 44., 2.),
                        Rect::new(x - 22., y + 22., 44., 2.),
                        Rect::new(x - 22., y - 22., 2., 44.),
                        Rect::new(x + 22., y - 22., 2., 44.),
                    ] {
                        paint.rect(r, color);
                    }
                    paint.text(
                        [x - 25., y + 30.],
                        &format!("BANDIT {}", marker.number),
                        1.,
                        color,
                    );
                    paint.text(
                        [x - 25., y + 45.],
                        &format!("{} M", marker.distance),
                        1.,
                        WHITE,
                    );
                    paint.rect(
                        Rect::new(x - 22., y - 29., 44. * marker.health as f32 / 100., 3.),
                        color,
                    );
                }
                let radar = if hud.radar_locked {
                    "RADAR LOCK".into()
                } else if hud.radar_progress > 0 {
                    format!("LOCKING {}%", hud.radar_progress)
                } else {
                    "RADAR SEARCH".into()
                };
                paint.text([500., 74.], &radar, 1.5, CYAN);
                if hud.incoming_missile {
                    paint.text([480., 120.], "MISSILE INBOUND", 2., [1., 0.13, 0.04, 1.]);
                } else if hud.incoming_lock {
                    paint.text([480., 120.], "RADAR LOCK WARNING", 2., [1., 0.45, 0.05, 1.]);
                }
                if hud.outside {
                    paint.text(
                        [400., 170.],
                        "RETURN TO COMBAT AREA",
                        2.,
                        [1., 0.45, 0.05, 1.],
                    );
                }
                paint.text([460., 680.], "F WEAPON   SPACE FIRE   P PAUSE", 1.2, WHITE);
            }
            Screen::Pause => {
                paint.rect(Rect::new(0., 0., 1280., 720.), [0.005, 0.01, 0.025, 0.7]);
                paint.panel(Rect::new(440., 90., 400., 516.));
                paint.text([480., 116.], "PAUSE MENU", 3., WHITE);
                paint.text([480., 150.], "P / RESUME", 1., MUTED);
                for (i, (label, action)) in [
                    ("RESUME", Action::Resume),
                    ("SETTINGS", Action::Settings),
                    ("RESPAWN", Action::Respawn),
                    ("END MATCH", Action::EndMatch),
                    ("SIMULATE LINK LOSS", Action::Disconnect),
                    ("LEAVE MATCH", Action::Leave),
                ]
                .into_iter()
                .enumerate()
                {
                    paint.enabled_button(
                        Rect::new(468., 190. + i as f32 * 64., 344., 46.),
                        label,
                        action,
                        action != Action::EndMatch
                            || !self.online_game
                            || self.session.as_ref().is_some_and(|s| s.host),
                    );
                }
            }
            Screen::Report => {
                paint.rect(Rect::new(0., 0., 1280., 720.), [0.005, 0.01, 0.025, 0.75]);
                paint.header("MATCH REPORT", "AFTER ACTION REPORT");
                paint.panel(Rect::new(240., 180., 800., 330.));
                paint.text([280., 220.], "MISSION COMPLETE", 3., CYAN);
                paint.text(
                    [280., 288.],
                    "PILOT           KILLS     DEATHS     SCORE",
                    1.5,
                    MUTED,
                );
                paint.text(
                    [280., 338.],
                    &format!(
                        "YOU             {:4}       {:4}      {:4}",
                        self.hud.kills,
                        self.hud.deaths,
                        self.hud.kills * 100
                    ),
                    1.5,
                    WHITE,
                );
                paint.text(
                    [280., 398.],
                    &format!("SHOTS: {}   HITS: {}", self.hud.shots, self.hud.hits),
                    1.5,
                    MUTED,
                );
                paint.button(
                    Rect::new(240., 548., 384., 54.),
                    "CONTINUE",
                    Action::ReportContinue,
                );
                paint.button(
                    Rect::new(656., 548., 384., 54.),
                    "SERVER BROWSER",
                    Action::Leave,
                );
            }
        }
        if self.screen != Screen::Gameplay {
            paint.text(
                [64., 688.],
                "ARROWS  NAVIGATE / ADJUST     ENTER  SELECT     ESC  BACK",
                1.2,
                MUTED,
            );
        }
        if !paint.widgets.is_empty() {
            self.focused = self.focused.min(paint.widgets.len() - 1);
            if !paint.widgets[self.focused].enabled {
                self.focused = paint.widgets.iter().position(|w| w.enabled).unwrap_or(0);
            }
        }
        // Add widget visuals last so their labels stay above panels and backgrounds.
        for (i, widget) in paint.widgets.iter().enumerate() {
            let active = i == self.focused;
            let color = if !widget.enabled {
                [0.025, 0.035, 0.045, 0.92]
            } else if self.pressed == Some(i) {
                [0.04, 0.22, 0.27, 0.97]
            } else if active {
                [0.035, 0.13, 0.18, 0.96]
            } else {
                [0.018, 0.035, 0.055, 0.94]
            };
            paint.commands.push(DrawCommand::Rect(widget.rect, color));
            if active {
                paint.commands.push(DrawCommand::Rect(
                    Rect::new(widget.rect.x, widget.rect.y, 3., widget.rect.h),
                    CYAN,
                ));
            }
            let scale = widget.label_scale();
            paint.commands.push(DrawCommand::Text {
                position: [
                    widget.rect.x + 20.,
                    widget.rect.y + (widget.rect.h - 8. * scale) * 0.5,
                ],
                text: widget.label.clone(),
                scale,
                color: if !widget.enabled {
                    MUTED
                } else if active {
                    CYAN
                } else {
                    WHITE
                },
            });
        }
        // Server metadata is drawn over row backgrounds.
        if self.screen == Screen::ServerBrowser {
            for (i, (_, _, info)) in self.browser_rows().iter().enumerate() {
                paint.text([578., 240. + i as f32 * 76.], info, 1.5, MUTED);
            }
        }
        self.widgets = paint.widgets;
        self.frame.commands = paint.commands;
        self.frame.show_world = world;
        self.frame.world_region =
            (self.screen == Screen::Spawn).then_some(Rect::new(640., 260., 600., 330.));
        self.frame.revision = self.frame.revision.wrapping_add(1);
        self.dirty = false;
    }
}

fn cycle(value: usize, delta: i32, count: usize) -> usize {
    (value as i32 + delta).rem_euclid(count as i32) as usize
}

pub fn to_design(point: [f32; 2], viewport: [u32; 2]) -> Option<[f32; 2]> {
    let scale = (viewport[0] as f32 / DESIGN_SIZE[0]).min(viewport[1] as f32 / DESIGN_SIZE[1]);
    if scale <= 0. {
        return None;
    }
    let offset = [
        (viewport[0] as f32 - DESIGN_SIZE[0] * scale) * 0.5,
        (viewport[1] as f32 - DESIGN_SIZE[1] * scale) * 0.5,
    ];
    let point = [
        (point[0] - offset[0]) / scale,
        (point[1] - offset[1]) / scale,
    ];
    Rect::new(0., 0., DESIGN_SIZE[0], DESIGN_SIZE[1])
        .contains(point)
        .then_some(point)
}

const WHITE: [f32; 4] = [0.82, 0.9, 0.96, 1.];
const CYAN: [f32; 4] = [0.1, 0.8, 0.93, 1.];
const MUTED: [f32; 4] = [0.34, 0.46, 0.56, 1.];
const BORDER: [f32; 4] = [0.045, 0.12, 0.17, 1.];

#[derive(Default)]
struct Painter {
    commands: Vec<DrawCommand>,
    widgets: Vec<Widget>,
}

impl Painter {
    fn rect(&mut self, rect: Rect, color: [f32; 4]) {
        self.commands.push(DrawCommand::Rect(rect, color));
    }
    fn text(&mut self, position: [f32; 2], text: &str, scale: f32, color: [f32; 4]) {
        self.commands.push(DrawCommand::Text {
            position,
            text: text.to_uppercase(),
            scale,
            color,
        });
    }
    fn panel(&mut self, rect: Rect) {
        self.rect(rect, [0.008, 0.018, 0.035, 0.94]);
        self.rect(Rect::new(rect.x, rect.y, rect.w, 2.), BORDER);
    }
    fn background(&mut self, image: UiImage, world: bool) {
        if !world {
            self.commands.push(DrawCommand::Image(
                image,
                Rect::new(0., 0., 1280., 720.),
                [0.55, 0.64, 0.75, 1.],
            ));
            self.rect(Rect::new(0., 0., 1280., 720.), [0.003, 0.01, 0.025, 0.58]);
        }
        self.rect(Rect::new(0., 0., 1280., 7.), CYAN);
    }
    fn header(&mut self, title: &str, subtitle: &str) {
        self.text([64., 46.], title, 4., WHITE);
        self.text([66., 100.], subtitle, 1.5, CYAN);
    }
    fn notice(&mut self, message: &str) {
        let message: String = message.chars().take(106).collect();
        self.text([66., 616.], &message, 1.3, [0.85, 0.6, 0.17, 1.]);
    }
    fn button(&mut self, rect: Rect, label: &str, action: Action) {
        self.enabled_button(rect, label, action, true);
    }
    fn enabled_button(&mut self, rect: Rect, label: &str, action: Action, enabled: bool) {
        self.widgets.push(Widget {
            rect,
            label: label.into(),
            action,
            enabled,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guests_can_paste_links_and_hosts_do_not_need_an_address() {
        let mut ui = UiController::default();
        ui.enable_networking();
        ui.go(Screen::ServerBrowser);
        let link = "jetgame://join?v=1&endpoint=Example.net%3A29010";
        ui.paste_text(link);
        select(&mut ui, Action::ConnectAddress);
        assert!(
            matches!(ui.network_requests.back(),Some(NetworkRequest::Join { endpoint,.. }) if endpoint==link)
        );
        ui.leave();
        select(&mut ui, Action::Host);
        ui.ensure_layout();
        assert_eq!(ui.widgets.len(), 6);
        select(&mut ui, Action::Create);
        assert!(matches!(
            ui.network_requests.back(),
            Some(NetworkRequest::Host { .. })
        ));
    }

    #[test]
    fn invitation_copy_is_cleared_when_the_connection_is_lost() {
        let mut ui = UiController::default();
        ui.enable_networking();
        ui.go(Screen::Lobby);
        ui.network_event(NetworkEvent::ShareLink {
            op: 0,
            link: "jetgame://join?v=1&endpoint=localhost:29010".into(),
            scope: LinkScope::Local,
            warning: String::new(),
        });
        select(&mut ui, Action::CopyLink);
        assert!(ui.take_copy_text().unwrap().starts_with("jetgame://"));
        ui.session_event(SessionEvent::ConnectionLost("test".into()));
        assert!(ui.share_link.is_empty());
        assert_eq!(ui.screen(), Screen::ServerBrowser);
    }

    const SCREENS: [Screen; 11] = [
        Screen::Splash,
        Screen::Start,
        Screen::Settings,
        Screen::ServerBrowser,
        Screen::Joining,
        Screen::Host,
        Screen::Lobby,
        Screen::Spawn,
        Screen::Gameplay,
        Screen::Pause,
        Screen::Report,
    ];

    fn select(ui: &mut UiController, action: Action) {
        ui.ensure_layout();
        ui.focused = ui
            .widgets
            .iter()
            .position(|w| w.action == action)
            .expect("visible action");
        ui.key(UiKey::Enter);
    }

    #[test]
    fn complete_guest_route_uses_keyboard_navigation_and_pause() {
        let mut ui = UiController::default();
        ui.key(UiKey::Enter);
        assert_eq!(ui.screen(), Screen::Start);
        ui.key(UiKey::Enter);
        assert_eq!(ui.screen(), Screen::ServerBrowser);
        for _ in 0..4 {
            ui.key(UiKey::Down);
        }
        ui.key(UiKey::Enter);
        assert_eq!(ui.screen(), Screen::Joining);
        ui.tick(Duration::from_secs(2));
        assert_eq!(ui.screen(), Screen::Lobby);
        ui.key(UiKey::Enter); // Ready
        ui.key(UiKey::Down);
        ui.key(UiKey::Enter); // Start preview
        assert_eq!(ui.screen(), Screen::Spawn);
        for _ in 0..2 {
            ui.key(UiKey::Down);
        }
        ui.key(UiKey::Enter);
        assert_eq!(ui.screen(), Screen::Gameplay);
        assert!(!ui.key(UiKey::Up)); // future flight controls receive unconsumed keys
        ui.key(UiKey::Pause);
        assert_eq!(ui.screen(), Screen::Pause);
        ui.key(UiKey::Pause);
        assert_eq!(ui.screen(), Screen::Gameplay);
        ui.session_event(SessionEvent::AircraftDestroyed);
        assert_eq!(ui.screen(), Screen::Spawn);
        ui.session_event(SessionEvent::MatchEnded);
        assert_eq!(ui.screen(), Screen::Report);
        ui.key(UiKey::Enter);
        assert_eq!(ui.screen(), Screen::Lobby);
    }

    #[test]
    fn joining_an_active_match_goes_directly_to_spawn() {
        let mut ui = UiController::default();
        ui.go(Screen::ServerBrowser);
        select(&mut ui, Action::Server(1));
        select(&mut ui, Action::Join);
        ui.tick(Duration::from_secs(2));
        assert_eq!(ui.screen(), Screen::Spawn);
    }

    #[test]
    fn host_cancel_and_host_report_use_documented_destinations() {
        let mut ui = UiController::default();
        ui.go(Screen::Host);
        select(&mut ui, Action::Back);
        assert_eq!(ui.screen(), Screen::ServerBrowser);
        select(&mut ui, Action::Host);
        select(&mut ui, Action::Create);
        assert!(ui.session.as_ref().unwrap().host);
        select(&mut ui, Action::StartMatch);
        ui.session_event(SessionEvent::MatchEnded);
        select(&mut ui, Action::ReportContinue);
        assert_eq!(ui.screen(), Screen::Host);
    }

    #[test]
    fn host_configuration_is_carried_into_lobby() {
        let mut ui = UiController::default();
        ui.go(Screen::Host);
        select(&mut ui, Action::Mode);
        select(&mut ui, Action::Map);
        select(&mut ui, Action::MaxPlayers);
        select(&mut ui, Action::Create);
        let session = ui.session.as_ref().unwrap();
        assert_eq!((session.mode, session.map, session.max_players), (1, 1, 16));
    }

    #[test]
    fn disconnect_overrides_every_screen_and_clears_session() {
        for screen in SCREENS {
            let mut ui = UiController::default();
            ui.start_preview();
            ui.go(screen);
            ui.session_event(SessionEvent::ConnectionLost("test".into()));
            assert_eq!(ui.screen(), Screen::ServerBrowser, "{screen:?}");
            assert!(ui.session.is_none());
            assert!(ui.message.contains("CONNECTION LOST"));
        }
    }

    #[test]
    fn left_right_adjust_settings_and_values_survive_navigation() {
        let mut ui = UiController::default();
        ui.go(Screen::Start);
        select(&mut ui, Action::Settings);
        ui.key(UiKey::Right);
        assert_eq!(ui.settings.volume, 80);
        for _ in 0..30 {
            ui.key(UiKey::Left);
        }
        assert_eq!(ui.settings.volume, 0);
        select(&mut ui, Action::Vsync);
        assert!(ui.settings.vsync);
        ui.key(UiKey::Back);
        assert_eq!(ui.screen(), Screen::Start);
        select(&mut ui, Action::Settings);
        assert_eq!(ui.settings.volume, 0);
        select(&mut ui, Action::Reset);
        assert_eq!(ui.settings.volume, 75);
    }

    #[test]
    fn pause_settings_return_to_pause_and_resume_to_gameplay() {
        let mut ui = UiController::default();
        ui.start_preview();
        ui.key(UiKey::Pause);
        select(&mut ui, Action::Settings);
        assert!(ui.frame().show_world);
        ui.key(UiKey::Back);
        assert_eq!(ui.screen(), Screen::Pause);
        ui.key(UiKey::Enter);
        assert_eq!(ui.screen(), Screen::Gameplay);
    }

    #[test]
    fn scaled_mouse_click_and_enter_activate_the_same_widget() {
        let mut mouse = UiController::default();
        let mut keyboard = UiController::default();
        mouse.go(Screen::Start);
        keyboard.go(Screen::Start);
        mouse.pointer_move([372., 447.], [1920, 1080]);
        mouse.pointer_button(true);
        mouse.pointer_button(false);
        keyboard.key(UiKey::Enter);
        assert_eq!(mouse.screen(), keyboard.screen());
        assert_eq!(mouse.screen(), Screen::ServerBrowser);
    }

    #[test]
    fn releasing_outside_button_does_not_activate_it() {
        let mut ui = UiController::default();
        ui.go(Screen::Start);
        ui.pointer_move([248., 298.], [1280, 720]);
        ui.pointer_button(true);
        ui.pointer_move([1200., 600.], [1280, 720]);
        ui.pointer_button(false);
        assert_eq!(ui.screen(), Screen::Start);
    }

    #[test]
    fn mouse_left_chevron_decreases_stepper() {
        let mut ui = UiController::default();
        ui.go(Screen::Settings);
        ui.ensure_layout();
        let widget = &ui.widgets[0];
        let x = widget.rect.x
            + 20.
            + (widget.label.find('<').unwrap() as f32 + 0.5) * 8. * widget.label_scale();
        let point = [x, widget.rect.y + widget.rect.h * 0.5];
        ui.pointer_move(point, [1280, 720]);
        ui.pointer_button(true);
        ui.pointer_button(false);
        assert_eq!(ui.settings.volume, 70);
        ui.key(UiKey::Enter);
        assert_eq!(ui.settings.volume, 75);
    }

    #[test]
    fn disabled_lobby_action_is_skipped() {
        let mut ui = UiController {
            session: Some(Session {
                host: false,
                name: "test".into(),
                ready: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        ui.go(Screen::Lobby);
        ui.key(UiKey::Down);
        assert_eq!(ui.widgets[ui.focused].action, Action::Leave);
    }

    #[test]
    fn host_name_text_is_bounded_editable_and_has_empty_fallback() {
        let mut ui = UiController::default();
        ui.go(Screen::Host);
        ui.key(UiKey::Enter);
        for _ in 0..30 {
            ui.key(UiKey::Backspace);
        }
        ui.text("pilot p");
        assert_eq!(ui.host_name, "PILOT P");
        ui.text("012345678901234567890123456789");
        assert_eq!(ui.host_name.len(), 24);
        for _ in 0..30 {
            ui.key(UiKey::Backspace);
        }
        ui.key(UiKey::Enter);
        assert!(!ui.editing_text());
        select(&mut ui, Action::Create);
        assert_eq!(ui.session.as_ref().unwrap().name, "LOCAL DOGFIGHT");
    }

    #[test]
    fn unchanged_menus_retain_their_display_list() {
        let mut ui = UiController::default();
        ui.go(Screen::Start);
        let revision = ui.frame().revision;
        ui.tick(Duration::from_secs(30));
        assert!(!ui.needs_redraw());
        assert_eq!(ui.frame().revision, revision);
    }

    #[test]
    fn network_join_waits_for_real_events_and_cancel_ignores_stale_success() {
        let mut ui = UiController::default();
        ui.enable_networking();
        ui.connect_direct("127.0.0.1:29010".into());
        let old = ui.network_operation;
        ui.tick(Duration::from_secs(10));
        assert_eq!(ui.screen(), Screen::Joining);
        ui.key(UiKey::Back);
        assert_eq!(ui.screen(), Screen::ServerBrowser);
        ui.network_event(NetworkEvent::Connected {
            op: old,
            endpoint: "127.0.0.1:29010".into(),
            info: crate::networking::SessionInfo {
                name: "STALE".into(),
                player_id: 1,
                host: false,
                max_players: 8,
                map: MAPS[0].into(),
                mode: MODES[0].into(),
                capabilities: vec!["lobby".into()],
                players: vec![],
            },
        });
        assert_eq!(ui.screen(), Screen::ServerBrowser);
        assert!(ui.session.is_none());
    }

    #[test]
    fn network_start_waits_for_setup_and_spawns_through_authority() {
        let mut ui = UiController::default();
        ui.enable_networking();
        ui.session = Some(Session {
            host: true,
            ready: true,
            ..Default::default()
        });
        ui.go(Screen::Lobby);
        select(&mut ui, Action::StartMatch);
        assert_eq!(ui.screen(), Screen::Lobby);
        assert!(matches!(
            ui.network_requests.back(),
            Some(NetworkRequest::Start { .. })
        ));
        ui.network_request_failed("All players must be ready".into());
        assert_eq!(ui.screen(), Screen::Lobby);
        let config = crate::config::GameConfig::default();
        ui.network_event(NetworkEvent::GameStarted {
            op: 0,
            setup: crate::game::net_types::MatchSetup {
                epoch: 7,
                capacity: 2,
                rules: config.rules,
                flight: config.flight,
                sync: config.sync,
                level_signature: 0,
                free_for_all: false,
            },
            players: vec![],
        });
        assert_eq!(ui.screen(), Screen::Spawn);
        select(&mut ui, Action::Spawn);
        assert!(matches!(
            ui.network_requests.back(),
            Some(NetworkRequest::Action {
                epoch: 7,
                action: GameAction::Spawn,
                action_id: 1,
                ..
            })
        ));
        ui.network_event(NetworkEvent::ActionResult {
            op: 0,
            epoch: 6,
            action_id: 1,
            accepted: false,
            reason: "old round".into(),
        });
        assert_eq!(ui.screen(), Screen::Gameplay);
        ui.network_event(NetworkEvent::ActionResult {
            op: 0,
            epoch: 7,
            action_id: 1,
            accepted: false,
            reason: "Respawn cooldown".into(),
        });
        assert_eq!(ui.screen(), Screen::Spawn);
    }

    #[test]
    fn network_browser_pages_and_disables_join_when_empty() {
        let mut ui = UiController::default();
        ui.enable_networking();
        ui.go(Screen::ServerBrowser);
        ui.ensure_layout();
        assert!(
            !ui.widgets
                .iter()
                .find(|w| w.action == Action::Join)
                .unwrap()
                .enabled
        );
        ui.network_event(NetworkEvent::Servers {
            op: 0,
            servers: (0..8)
                .map(|i| ServerListing {
                    id: i.to_string(),
                    name: format!("SERVER {i}"),
                    endpoint: format!("127.0.0.1:{}", 29010 + i),
                    players: 0,
                    max_players: 8,
                    in_progress: false,
                    reachability: "unverified".into(),
                })
                .collect(),
        });
        select(&mut ui, Action::NextPage);
        assert_eq!(ui.browser_rows()[0].0, 3);
    }

    #[test]
    fn all_screens_have_valid_nonoverlapping_widgets() {
        let mut ui = UiController::default();
        ui.start_preview();
        for screen in SCREENS {
            ui.go(screen);
            ui.ensure_layout();
            for (i, widget) in ui.widgets.iter().enumerate() {
                let a = widget.rect;
                assert!(
                    a.x >= 0. && a.y >= 0. && a.x + a.w <= 1280. && a.y + a.h <= 720.,
                    "{screen:?}"
                );
                for other in &ui.widgets[i + 1..] {
                    let b = other.rect;
                    assert!(
                        a.x + a.w <= b.x
                            || b.x + b.w <= a.x
                            || a.y + a.h <= b.y
                            || b.y + b.h <= a.y,
                        "{screen:?}"
                    );
                }
            }
        }
    }
}
