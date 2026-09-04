//! Embersong desktop host: Bevy isometric renderer + wasmtime WASM guest.
//!
//! `--headless` runs a scripted sim with no window (CI smoke test, exercises
//! the WASM backend + synth without an audio device).

mod audio;
mod backend;
mod sprites;

use audio::SoundBus;
use backend::Backend;
use bevy::prelude::*;
use embersong_core::{Action, Event, Game, MonsterKind, Phase, Song};
use sprites::{build_sprites, SpriteSet};
use std::sync::Mutex;

const SAVE_PATH: &str = "embersong-save.json";
const TILE_W: f32 = 46.0;
const TILE_H: f32 = 23.0;
const MAP_W: i32 = 20;
const MAP_H: i32 = 14;

const SONGS: [Song; 3] = [Song::EmberReel, Song::MothLullaby, Song::WardensHymn];

// ---------------------------------------------------------------------------
// App state & resources
// ---------------------------------------------------------------------------

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum AppState {
    #[default]
    Title,
    Explore,
    Combat,
    End,
}

#[derive(Resource)]
struct BackendRes(Mutex<Backend>);

#[derive(Resource)]
struct Session {
    game: Game,
    log: Vec<String>,
    player: IVec2,
    map: GenMap,
    song_idx: usize,
}

#[derive(Clone)]
struct GenMap {
    vault: usize,
    pillars: Vec<IVec2>,
    dens: Vec<(IVec2, MonsterKind)>,
    stairs: IVec2,
}

impl GenMap {
    fn generate(seed: u64, vault: usize, kinds: &[MonsterKind]) -> Self {
        // Deterministic scatter from seed+vault (visual only; rules live in core).
        let mut s = seed ^ ((vault as u64 + 1) * 0x9e3779b97f4a7c15);
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut pillars = Vec::new();
        for _ in 0..14 {
            let p = IVec2::new(
                1 + (next() % (MAP_W - 2) as u64) as i32,
                1 + (next() % (MAP_H - 2) as u64) as i32,
            );
            if (p.x - MAP_W / 2).abs() + (p.y - MAP_H / 2).abs() > 3 {
                pillars.push(p);
            }
        }
        let mut dens = Vec::new();
        for (i, k) in kinds.iter().take(5).enumerate() {
            let p = IVec2::new(
                2 + ((next() + i as u64 * 7) % (MAP_W - 4) as u64) as i32,
                2 + ((next() + i as u64 * 13) % (MAP_H - 4) as u64) as i32,
            );
            if !pillars.contains(&p) {
                dens.push((p, *k));
            }
        }
        Self {
            vault,
            pillars,
            dens,
            stairs: IVec2::new(MAP_W - 2, MAP_H - 2),
        }
    }

    fn walkable(&self, p: IVec2) -> bool {
        // Borders render as walls, so they block like pillars do.
        p.x > 0 && p.y > 0 && p.x < MAP_W - 1 && p.y < MAP_H - 1 && !self.pillars.contains(&p)
    }
}

// --- markers ---------------------------------------------------------------

#[derive(Component)]
struct TitleScreen;
#[derive(Component)]
struct ExploreScreen;
#[derive(Component)]
struct CombatScreen;
#[derive(Component)]
struct EndScreen;
/// Any tile/token entity of the map layer (rebuilt on vault change).
#[derive(Component)]
struct MapTile;
#[derive(Component)]
struct HudMain;
#[derive(Component)]
struct HudLog;
#[derive(Component)]
struct SongLabel;
#[derive(Component)]
struct FoeName;
#[derive(Component)]
struct CombatFoe;
#[derive(Component)]
struct PlayerToken;
#[derive(Component)]
struct PlayerGlow;
#[derive(Component)]
struct Ember {
    vel: f32,
    sway: f32,
    phase: f32,
}

#[derive(Component)]
struct CombatBtn(BtnKind);

#[derive(Clone, Copy, PartialEq, Eq)]
enum BtnKind {
    Strike,
    Song,
    Soothe,
    Heal,
    Summon,
    Flee,
}

// ---------------------------------------------------------------------------
// Headless CI sim
// ---------------------------------------------------------------------------

fn headless(seed: u64, turns: u32, native: bool) -> i32 {
    let mut backend = if native {
        Backend::Native
    } else {
        Backend::prefer_wasm()
    };
    println!("embersong headless: backend={} seed={seed}", backend.name());
    let mut game = Game::new(seed);
    // Differential mode: run a WASM guest in lockstep with native and fail on
    // the first divergence (proves the sandbox runs identical rules).
    let verify = std::env::args().any(|a| a == "--verify");
    let mut wasm_backend = if verify {
        match backend::WasmGuest::load() {
            Ok(g) => Some(backend::Backend::Wasm(g)),
            Err(e) => {
                eprintln!("--verify needs core.wasm: {e}");
                std::process::exit(2);
            }
        }
    } else {
        None
    };
    let mut wasm_game = if verify { Some(Game::new(seed)) } else { None };
    let log_skip: u32 = std::env::var("EMBERSONG_LOG_SKIP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let log_lines: u32 = std::env::var("EMBERSONG_LOG_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let dump_path: Option<String> = parse_arg("--dump");
    if let Some(p) = &dump_path {
        let _ = std::fs::remove_file(p);
    }
    for played in 0..turns {
        if game.phase == Phase::GameOver || game.phase == Phase::Victory {
            break;
        }
        let act = if game.hero.hp <= (game.hero.max_hp as f32 * 0.4) as i32 {
            Action::Heal
        } else if game
            .current
            .as_ref()
            .map(|m| m.harmony > 40)
            .unwrap_or(false)
        {
            Action::Soothe
        } else {
            Action::Strike
        };
        let events = backend.act(&mut game, act);
        if let (Some(wb), Some(wg)) = (wasm_backend.as_mut(), wasm_game.as_mut()) {
            let w_events = wb.act(wg, act);
            let (j1, j2) = (game.to_json(), wg.to_json());
            if j1 != j2 {
                eprintln!("DIVERGENCE at turn {played} action {act:?}");
                eprintln!("  native events: {events:?}");
                eprintln!("  wasm   events: {w_events:?}");
                std::fs::write("/tmp/opencode/diverge-native.json", &j1).ok();
                std::fs::write("/tmp/opencode/diverge-wasm.json", &j2).ok();
                eprintln!("  states written to /tmp/opencode/diverge-*.json");
                std::process::exit(3);
            }
        }
        if let Some(p) = &dump_path {
            dump_turn(p, played, &game);
        }
        // Exercise the synth for every sound (rendered, not played).
        for e in events {
            if let Event::Sound(t) = e {
                let s = embersong_synth::render_sfx(t);
                assert!(!s.samples.is_empty());
            }
            if let Event::Message(m) = e {
                if played >= log_skip && played < log_skip + log_lines {
                    println!("  t{played}: {m}");
                }
            }
        }
    }
    println!(
        "done: phase={:?} turns={} binds={} vault={} backend={}",
        game.phase,
        game.hero.turns.max(0),
        game.hero.score,
        Game::vaults()[game.vault_index].name,
        backend.name()
    );
    0
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn iso(p: IVec2) -> Vec2 {
    Vec2::new(
        (p.x - p.y) as f32 * TILE_W / 2.0,
        -((p.x + p.y) as f32) * TILE_H / 2.0 + 150.0,
    )
}

fn font(asset_server: &AssetServer, size: f32, color: Color) -> (TextFont, TextColor) {
    (
        TextFont {
            font: asset_server.load("fonts/body.ttf"),
            font_size: size,
            ..default()
        },
        TextColor(color),
    )
}

fn push_log(session: &mut Session, line: String) {
    session.log.push(line);
    if session.log.len() > 40 {
        let excess = session.log.len() - 40;
        session.log.drain(0..excess);
    }
}

fn apply_events(session: &mut Session, bus: &SoundBus, events: Vec<Event>) {
    for e in events {
        match e {
            Event::Message(m) => push_log(session, m),
            Event::Sound(t) => bus.play_trigger(t),
            Event::Tamed(k) => push_log(
                session,
                format!("{} glows in your Bestiary.", k.display_name()),
            ),
            Event::Evolved { from, into } => push_log(
                session,
                format!("{} → {}!", from.display_name(), into.display_name()),
            ),
            Event::VaultCleared(i) => {
                push_log(session, format!("✦ {} retuned! ✦", Game::vaults()[i].name))
            }
        }
    }
}

fn save_game(game: &Game) {
    let _ = std::fs::write(SAVE_PATH, game.to_json());
}

fn hero_line(game: &Game) -> String {
    let vault = &Game::vaults()[game.vault_index];
    format!(
        "HP {}/{}   Breath {}/{}   Binds {}   Shards {}   Companions {}   Bestiary {}/12\n{} (vault {}/{})",
        game.hero.hp,
        game.hero.max_hp,
        game.hero.breath,
        game.hero.max_breath,
        game.hero.score,
        game.hero.shards,
        game.hero.companions.len(),
        game.hero.bestiary.len(),
        vault.name,
        game.vault_index + 1,
        Game::vaults().len()
    )
}

fn foe_line(game: &Game) -> String {
    match &game.current {
        Some(m) => format!(
            "☠ {}  HP {}/{}   Harmony {}/{}",
            m.name,
            m.hp,
            m.max_hp,
            m.harmony,
            m.kind.base().tame_threshold
        ),
        None => "No foe stirs...".to_string(),
    }
}

fn kinds_or_current(game: &Game) -> Vec<MonsterKind> {
    let mut kinds = game.queue.clone();
    if let Some(m) = &game.current {
        kinds.push(m.kind);
    }
    kinds
}

fn song_name(session: &Session) -> &'static str {
    SONGS[session.song_idx].name()
}

/// Run one hero turn through the backend; true when the run ended.
fn do_hero_action(
    session: &mut Session,
    backend: &BackendRes,
    bus: &SoundBus,
    action: Action,
    next: &mut NextState<AppState>,
) {
    let mut backend = backend.0.lock().unwrap();
    let events = backend.act(&mut session.game, action);
    drop(backend);
    apply_events(session, bus, events);
    save_game(&session.game);
    if matches!(session.game.phase, Phase::GameOver | Phase::Victory) {
        let _ = std::fs::remove_file(SAVE_PATH);
        next.set(AppState::End);
    }
}

// ---------------------------------------------------------------------------
// Setup: camera + embers
// ---------------------------------------------------------------------------

fn setup_once(mut commands: Commands) {
    commands.spawn(Camera2d);
    // Drifting embers for wonder (persist across states).
    let mut s = 0x12345678u64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let palette = [
        Color::srgba(1.0, 0.6, 0.25, 0.5),
        Color::srgba(0.5, 0.85, 1.0, 0.4),
        Color::srgba(0.8, 0.55, 1.0, 0.35),
    ];
    for i in 0..70 {
        let x = (next() % 1280) as f32 - 640.0;
        let y = (next() % 800) as f32 - 400.0;
        commands.spawn((
            Sprite {
                color: palette[i % 3],
                custom_size: Some(Vec2::new(3.0, 3.0)),
                ..default()
            },
            Transform::from_xyz(x, y, 50.0),
            Ember {
                vel: 8.0 + (next() % 20) as f32,
                sway: 0.5 + (next() % 10) as f32 * 0.1,
                phase: (next() % 628) as f32 / 100.0,
            },
        ));
    }
}

fn ember_drift(time: Res<Time>, mut q: Query<(&mut Transform, &Ember)>) {
    for (mut t, e) in &mut q {
        t.translation.y += e.vel * time.delta_secs();
        t.translation.x +=
            (time.elapsed_secs() * e.sway + e.phase).sin() * 12.0 * time.delta_secs();
        if t.translation.y > 420.0 {
            t.translation.y = -420.0;
        }
    }
}

// ---------------------------------------------------------------------------
// Title
// ---------------------------------------------------------------------------

fn title_setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    let (f_big, c_big) = font(&asset_server, 44.0, Color::srgb(1.0, 0.85, 0.55));
    let (f_med, c_med) = font(&asset_server, 20.0, Color::srgb(0.85, 0.8, 0.95));
    let (f_dim, c_dim) = font(&asset_server, 17.0, Color::srgb(0.6, 0.58, 0.7));
    let cont = if std::path::Path::new(SAVE_PATH).exists() {
        "[ENTER] begin anew    [C] continue     — WASD move · E seek foe · R rest"
    } else {
        "[ENTER] begin    — WASD move · E seek foe · R rest"
    };
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            TitleScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new("✦ EMBERSONG ✦"), f_big, c_big));
            p.spawn((Text::new("Canticle of the Caged Stars"), f_med.clone(), c_med));
            p.spawn((
                Text::new("A Lantern-Bard's tale: steel, songs, and bound star-spawn.\nFight them — or hum them home into your Bestiary."),
                f_dim.clone(),
                c_dim,
            ));
            p.spawn((Text::new(cont), f_med.clone(), c_med));
            p.spawn((
                Text::new("combat: 1 strike · 2 song · 3 soothe · 4 heal · 5 summon · 6 flee · TAB verses"),
                f_dim,
                c_dim,
            ));
        });
}

fn begin_run(session: &mut Session, bus: &SoundBus, game: Game, next: &mut NextState<AppState>) {
    session.game = game;
    session.log.clear();
    session.player = IVec2::new(MAP_W / 2, MAP_H / 2);
    let kinds = kinds_or_current(&session.game);
    session.map = GenMap::generate(session.game.seed, session.game.vault_index, &kinds);
    push_log(
        session,
        "The lantern is lit. Hollow Hill waits.".to_string(),
    );
    let bed = embersong_synth::render_song_bed(2, false);
    bus.play(bed.samples, bed.rate);
    next.set(AppState::Explore);
}

fn title_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    bus: Res<SoundBus>,
    mut next: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Enter) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(1)
            % 1_000_000;
        begin_run(&mut session, &bus, Game::new(seed), &mut next);
    } else if keys.just_pressed(KeyCode::KeyC) {
        if let Ok(text) = std::fs::read_to_string(SAVE_PATH) {
            if let Some(game) = Game::from_json(&text) {
                begin_run(&mut session, &bus, game, &mut next);
            }
        }
    }
}

fn despawn_all<T: Component>(mut commands: Commands, q: Query<Entity, With<T>>) {
    for e in &q {
        commands.entity(e).despawn_recursive();
    }
}

// ---------------------------------------------------------------------------
// Explore
// ---------------------------------------------------------------------------

/// Spawn floor, walls, dens, stairs, player (all tagged `MapTile`).
fn spawn_map(commands: &mut Commands, session: &Session, sprites: &SpriteSet) {
    for x in 0..MAP_W {
        for y in 0..MAP_H {
            let p = IVec2::new(x, y);
            let border = x == 0 || y == 0 || x == MAP_W - 1 || y == MAP_H - 1;
            let pillar = session.map.pillars.contains(&p);
            let v = iso(p);
            if border || pillar {
                commands.spawn((
                    Sprite {
                        image: sprites.wall.clone(),
                        custom_size: Some(Vec2::new(TILE_W - 2.0, TILE_H * 1.7)),
                        ..default()
                    },
                    Transform::from_xyz(v.x, v.y + 8.0, v.y * 0.01 - 1.0),
                    ExploreScreen,
                    MapTile,
                ));
            } else {
                let tex = if (x + y) % 2 == 0 {
                    sprites.floor_a.clone()
                } else {
                    sprites.floor_b.clone()
                };
                commands.spawn((
                    Sprite {
                        image: tex,
                        custom_size: Some(Vec2::new(TILE_W - 2.0, TILE_H - 2.0)),
                        ..default()
                    },
                    Transform::from_xyz(v.x, v.y, v.y * 0.01 - 1.0),
                    ExploreScreen,
                    MapTile,
                ));
            }
        }
    }
    // Dens: element territory plate + the creature itself.
    for (p, kind) in session.map.dens.clone() {
        let v = iso(p);
        commands.spawn((
            Sprite {
                color: sprites::element_floor(kind.base().element),
                custom_size: Some(Vec2::new(30.0, 15.0)),
                ..default()
            },
            Transform::from_xyz(v.x, v.y - 2.0, 1.5),
            ExploreScreen,
            MapTile,
        ));
        commands.spawn((
            Sprite {
                image: sprites.creature(kind),
                custom_size: Some(Vec2::new(34.0, 40.0)),
                ..default()
            },
            Transform::from_xyz(v.x, v.y + 10.0, 2.0),
            ExploreScreen,
            MapTile,
        ));
    }
    {
        let v = iso(session.map.stairs);
        commands.spawn((
            Sprite {
                image: sprites.stairs.clone(),
                custom_size: Some(Vec2::new(30.0, 35.0)),
                ..default()
            },
            Transform::from_xyz(v.x, v.y + 8.0, 2.0),
            ExploreScreen,
            MapTile,
        ));
    }
    let pv = iso(session.player);
    commands.spawn((
        Sprite {
            image: sprites.glow.clone(),
            custom_size: Some(Vec2::new(72.0, 72.0)),
            ..default()
        },
        Transform::from_xyz(pv.x, pv.y, 2.5),
        ExploreScreen,
        MapTile,
        PlayerGlow,
    ));
    commands.spawn((
        Sprite {
            image: sprites.hero.clone(),
            custom_size: Some(Vec2::new(26.0, 36.0)),
            ..default()
        },
        Transform::from_xyz(pv.x, pv.y + 10.0, 3.0),
        ExploreScreen,
        MapTile,
        PlayerToken,
    ));
}

fn explore_setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut session: ResMut<Session>,
    sprites: Res<SpriteSet>,
) {
    let kinds = kinds_or_current(&session.game);
    session.map = GenMap::generate(session.game.seed, session.game.vault_index, &kinds);
    session.player = IVec2::new(MAP_W / 2, MAP_H / 2);
    spawn_map(&mut commands, &session, &sprites);

    let (f_main, c_main) = font(&asset_server, 17.0, Color::srgb(0.92, 0.9, 0.95));
    let (f_log, c_log) = font(&asset_server, 16.0, Color::srgb(0.75, 0.72, 0.85));
    let (f_help, c_help) = font(&asset_server, 15.0, Color::srgb(0.6, 0.58, 0.7));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(10.0),
                width: Val::Px(980.0),
                ..default()
            },
            ExploreScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new(""), f_main, c_main, HudMain));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(10.0),
                width: Val::Px(760.0),
                ..default()
            },
            ExploreScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new(""), f_log, c_log, HudLog));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                top: Val::Px(10.0),
                ..default()
            },
            ExploreScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new("WASD move · E seek foe · R rest"), f_help, c_help));
        });
}

#[allow(clippy::too_many_arguments)] // Bevy systems take their params as args.
fn explore_move(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    bus: Res<SoundBus>,
    mut next: ResMut<NextState<AppState>>,
    tiles: Query<Entity, With<MapTile>>,
    mut player_q: Query<&mut Transform, (With<PlayerToken>, Without<PlayerGlow>)>,
    mut glow_q: Query<&mut Transform, (With<PlayerGlow>, Without<PlayerToken>)>,
    sprites: Res<SpriteSet>,
) {
    // The core auto-advanced to the next vault after a clear: rebuild the map.
    if session.map.vault != session.game.vault_index {
        for e in &tiles {
            commands.entity(e).despawn_recursive();
        }
        let kinds = kinds_or_current(&session.game);
        session.map = GenMap::generate(session.game.seed, session.game.vault_index, &kinds);
        session.player = IVec2::new(MAP_W / 2, MAP_H / 2);
        spawn_map(&mut commands, &session, &sprites);
        return;
    }
    let mut step = IVec2::ZERO;
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::ArrowUp) {
        step.y -= 1;
    } else if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowDown) {
        step.y += 1;
    } else if keys.just_pressed(KeyCode::KeyA) || keys.just_pressed(KeyCode::ArrowLeft) {
        step.x -= 1;
    } else if keys.just_pressed(KeyCode::KeyD) || keys.just_pressed(KeyCode::ArrowRight) {
        step.x += 1;
    }
    if step != IVec2::ZERO {
        let dest = session.player + step;
        if session.map.walkable(dest) {
            session.player = dest;
            let v = iso(dest);
            for mut t in &mut player_q {
                t.translation.x = v.x;
                t.translation.y = v.y + 8.0;
            }
            for mut t in &mut glow_q {
                t.translation.x = v.x;
                t.translation.y = v.y;
            }
            if session.map.dens.iter().any(|(p, _)| *p == dest) {
                push_log(&mut session, "Something stirs in the dark...".to_string());
                next.set(AppState::Combat);
            }
        }
    }
    if keys.just_pressed(KeyCode::KeyE) || keys.just_pressed(KeyCode::Space) {
        if session.game.current.is_some() {
            let bed = embersong_synth::render_song_bed(1, true);
            bus.play(bed.samples, bed.rate);
            next.set(AppState::Combat);
        } else {
            push_log(&mut session, "Only embers drift here.".to_string());
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        session.game.hero.hp = session.game.hero.max_hp;
        session.game.hero.breath = session.game.hero.max_breath;
        push_log(
            &mut session,
            "You rest by the lantern-light. Fully healed.".to_string(),
        );
        bus.play_trigger(embersong_core::SoundTrigger::Heal);
        save_game(&session.game);
    }
}

fn explore_hud(
    session: Res<Session>,
    mut hud: Query<&mut Text, (With<HudMain>, Without<HudLog>)>,
    mut log: Query<&mut Text, With<HudLog>>,
) {
    let body = format!("{}\n{}", hero_line(&session.game), foe_line(&session.game));
    for mut t in &mut hud {
        t.0 = body.clone();
    }
    let tail: Vec<&str> = session
        .log
        .iter()
        .rev()
        .take(7)
        .map(|s| s.as_str())
        .collect();
    let joined = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    for mut t in &mut log {
        t.0 = joined.clone();
    }
}

// ---------------------------------------------------------------------------
// Combat
// ---------------------------------------------------------------------------

const BTN_NORMAL: Color = Color::srgb(0.20, 0.14, 0.32);
const BTN_HOVER: Color = Color::srgb(0.35, 0.22, 0.50);
const BTN_DOWN: Color = Color::srgb(0.50, 0.32, 0.65);

fn combat_setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    session: Res<Session>,
    sprites: Res<SpriteSet>,
) {
    // Dim full-screen backdrop (spawned first so it sits behind everything).
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.01, 0.06, 0.88)),
        CombatScreen,
    ));
    // The foe, large and centered in world space.
    let foe_tex = session
        .game
        .current
        .as_ref()
        .map(|m| sprites.creature(m.kind))
        .unwrap_or_else(|| sprites.hero.clone());
    commands.spawn((
        Sprite {
            image: foe_tex,
            custom_size: Some(Vec2::new(170.0, 198.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 110.0, 5.0),
        CombatScreen,
        CombatFoe,
    ));
    // Foe nameplate above the buttons.
    let (f_foe, c_foe) = font(&asset_server, 26.0, Color::srgb(1.0, 0.88, 0.6));
    let foe_name = session
        .game
        .current
        .as_ref()
        .map(|m| format!("☠ {} ☠", m.name))
        .unwrap_or_else(|| "No foe stirs...".to_string());
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(332.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            CombatScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new(foe_name), f_foe, c_foe, FoeName));
        });
    let labels = [
        (BtnKind::Strike, "1 · Strike"),
        (BtnKind::Song, "2 · Song"),
        (BtnKind::Soothe, "3 · Soothe"),
        (BtnKind::Heal, "4 · Heal"),
        (BtnKind::Summon, "5 · Summon"),
        (BtnKind::Flee, "6 · Flee"),
    ];
    // Command menu: pinned to the bottom-center so it never sits under the
    // top-left HUD text. Log panel ends at y=150 from the bottom; buttons
    // start at 270, clear of both.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(270.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(8.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            CombatScreen,
        ))
        .with_children(|p| {
            for (kind, label) in labels {
                let (f, c) = font(&asset_server, 16.0, Color::WHITE);
                p.spawn((
                    Button,
                    CombatBtn(kind),
                    Node {
                        width: Val::Px(128.0),
                        height: Val::Px(52.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(BTN_NORMAL),
                ))
                .with_children(|pp| {
                    if kind == BtnKind::Song {
                        pp.spawn((
                            Text::new(format!("{label}: {}", song_name(&session))),
                            f,
                            c,
                            SongLabel,
                        ));
                    } else {
                        pp.spawn((Text::new(label), f, c));
                    }
                });
            }
        });
    let (f_main, c_main) = font(&asset_server, 18.0, Color::srgb(0.93, 0.9, 0.96));
    let (f_log, c_log) = font(&asset_server, 16.0, Color::srgb(0.78, 0.74, 0.88));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(10.0),
                width: Val::Px(980.0),
                ..default()
            },
            CombatScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new(""), f_main, c_main, HudMain));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(10.0),
                width: Val::Px(800.0),
                ..default()
            },
            CombatScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new(""), f_log, c_log, HudLog));
        });
}

#[allow(clippy::type_complexity)] // Bevy HUD queries.
#[allow(clippy::too_many_arguments)] // Bevy systems take their params as args.
fn combat_hud(
    session: Res<Session>,
    sprites: Res<SpriteSet>,
    mut hud: Query<
        &mut Text,
        (
            With<HudMain>,
            Without<HudLog>,
            Without<SongLabel>,
            Without<FoeName>,
        ),
    >,
    mut log: Query<&mut Text, (With<HudLog>, Without<SongLabel>, Without<FoeName>)>,
    mut song: Query<&mut Text, (With<SongLabel>, Without<FoeName>)>,
    mut nameplate: Query<&mut Text, With<FoeName>>,
    mut foe: Query<&mut Sprite, With<CombatFoe>>,
    mut last_kind: Local<Option<MonsterKind>>,
) {
    let body = format!("{}\n{}", hero_line(&session.game), foe_line(&session.game));
    for mut t in &mut hud {
        t.0 = body.clone();
    }
    let tail: Vec<&str> = session
        .log
        .iter()
        .rev()
        .take(7)
        .map(|s| s.as_str())
        .collect();
    let joined = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    for mut t in &mut log {
        t.0 = joined.clone();
    }
    for mut t in &mut song {
        t.0 = format!("2 · Song: {}", song_name(&session));
    }
    // New foe stepped in (kill/bind): swap portrait + nameplate.
    let kind = session.game.current.as_ref().map(|m| m.kind);
    if kind != *last_kind {
        *last_kind = kind;
        let tex = kind
            .map(|k| sprites.creature(k))
            .unwrap_or_else(|| sprites.hero.clone());
        for mut s in &mut foe {
            s.image = tex.clone();
        }
        let name = session
            .game
            .current
            .as_ref()
            .map(|m| format!("☠ {} ☠", m.name))
            .unwrap_or_else(|| "No foe stirs...".to_string());
        for mut t in &mut nameplate {
            t.0 = name.clone();
        }
    }
}

fn combat_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    backend: Res<BackendRes>,
    bus: Res<SoundBus>,
    mut next: ResMut<NextState<AppState>>,
) {
    let mut act = None;
    if keys.just_pressed(KeyCode::Digit1) {
        act = Some(Action::Strike);
    } else if keys.just_pressed(KeyCode::Digit2) {
        act = Some(Action::Song(SONGS[session.song_idx]));
    } else if keys.just_pressed(KeyCode::Digit3) {
        act = Some(Action::Soothe);
    } else if keys.just_pressed(KeyCode::Digit4) {
        act = Some(Action::Heal);
    } else if keys.just_pressed(KeyCode::Digit5) {
        act = Some(Action::Summon(0));
    } else if keys.just_pressed(KeyCode::Digit6) {
        act = Some(Action::Flee);
    } else if keys.just_pressed(KeyCode::Tab) {
        session.song_idx = (session.song_idx + 1) % SONGS.len();
        let verse = song_name(&session);
        push_log(&mut session, format!("Verse ready: {verse}"));
    }
    if let Some(action) = act {
        do_hero_action(&mut session, &backend, &bus, action, &mut next);
    }
}

#[allow(clippy::type_complexity)] // Bevy button-interaction query.
fn combat_buttons(
    mut q: Query<
        (&Interaction, &mut BackgroundColor, &CombatBtn),
        (Changed<Interaction>, With<Button>),
    >,
    mut session: ResMut<Session>,
    backend: Res<BackendRes>,
    bus: Res<SoundBus>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (interaction, mut color, btn) in &mut q {
        match *interaction {
            Interaction::Pressed => {
                *color = BTN_DOWN.into();
                let action = match btn.0 {
                    BtnKind::Strike => Action::Strike,
                    BtnKind::Song => Action::Song(SONGS[session.song_idx]),
                    BtnKind::Soothe => Action::Soothe,
                    BtnKind::Heal => Action::Heal,
                    BtnKind::Summon => Action::Summon(0),
                    BtnKind::Flee => Action::Flee,
                };
                do_hero_action(&mut session, &backend, &bus, action, &mut next);
            }
            Interaction::Hovered => *color = BTN_HOVER.into(),
            Interaction::None => *color = BTN_NORMAL.into(),
        }
    }
}

fn foe_pulse(time: Res<Time>, mut q: Query<&mut Transform, With<CombatFoe>>) {
    let s = 1.0 + (time.elapsed_secs() * 3.0).sin() * 0.04;
    for mut t in &mut q {
        t.scale = Vec3::new(s, s, 1.0);
    }
}

// ---------------------------------------------------------------------------
// End
// ---------------------------------------------------------------------------

fn end_setup(mut commands: Commands, asset_server: Res<AssetServer>, session: Res<Session>) {
    let (title, color) = match session.game.phase {
        Phase::Victory => ("✦ ALL VAULTS SING ✦", Color::srgb(1.0, 0.9, 0.55)),
        _ if session.game.hero.hp <= 0 => ("THE LANTERN GUTTERS OUT", Color::srgb(0.9, 0.4, 0.4)),
        _ => ("YOU SLIP AWAY INTO THE MOSS", Color::srgb(0.7, 0.7, 0.9)),
    };
    let (f_big, c_big) = font(&asset_server, 40.0, color);
    let (f_med, c_med) = font(&asset_server, 20.0, Color::srgb(0.85, 0.82, 0.92));
    let summary = format!(
        "{} binds · {} turns · Bestiary {}/12 · Companions {}",
        session.game.hero.score,
        session.game.hero.turns.max(0),
        session.game.hero.bestiary.len(),
        session.game.hero.companions.len()
    );
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                ..default()
            },
            EndScreen,
        ))
        .with_children(|p| {
            p.spawn((Text::new(title), f_big, c_big));
            p.spawn((Text::new(summary), f_med.clone(), c_med));
            p.spawn((Text::new("[R] sing again"), f_med, c_med));
        });
}

fn end_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    bus: Res<SoundBus>,
    mut next: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(7)
            % 1_000_000;
        begin_run(&mut session, &bus, Game::new(seed), &mut next);
    }
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

fn parse_arg(flag: &str) -> Option<String> {
    std::env::args().skip_while(|a| a != flag).nth(1)
}

/// Append one JSON snapshot per turn (dev tool: diff native vs WASM streams).
fn dump_turn(path: &str, turn: u32, game: &Game) {
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    let _ = writeln!(f, "turn={turn} {}", game.to_json());
}

fn main() {
    if std::env::args().any(|a| a == "--headless") {
        let seed: u64 = parse_arg("--seed")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20260904);
        let turns: u32 = parse_arg("--turns")
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);
        let native = std::env::args().any(|a| a == "--native");
        std::process::exit(headless(seed, turns, native));
    }

    let backend = Backend::prefer_wasm();
    let label = backend.name().to_string();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1)
        % 1_000_000;
    let game = Game::new(seed);
    let kinds = kinds_or_current(&game);
    let map = GenMap::generate(seed, 0, &kinds);

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.03, 0.02, 0.08)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Embersong: Canticle of the Caged Stars".into(),
                resolution: (1280.0_f32, 800.0_f32).into(),
                ..default()
            }),
            ..default()
        }))
        .init_state::<AppState>()
        .insert_resource(Session {
            game,
            log: vec![format!("Rules engine: {label}")],
            player: IVec2::new(MAP_W / 2, MAP_H / 2),
            map,
            song_idx: 1,
        })
        .insert_resource(BackendRes(Mutex::new(backend)))
        .insert_resource(SoundBus::spawn())
        .add_systems(Startup, (setup_once, build_sprites))
        .add_systems(OnEnter(AppState::Title), title_setup)
        .add_systems(OnExit(AppState::Title), despawn_all::<TitleScreen>)
        .add_systems(OnEnter(AppState::Explore), explore_setup)
        .add_systems(OnExit(AppState::Explore), despawn_all::<ExploreScreen>)
        .add_systems(OnEnter(AppState::Combat), combat_setup)
        .add_systems(OnExit(AppState::Combat), despawn_all::<CombatScreen>)
        .add_systems(OnEnter(AppState::End), end_setup)
        .add_systems(OnExit(AppState::End), despawn_all::<EndScreen>)
        // One chain: total ordering silences cross-system borrow ambiguity.
        // Per-system states keep each screen's logic fenced to its screen.
        .add_systems(
            Update,
            (
                title_keys.run_if(in_state(AppState::Title)),
                explore_move.run_if(in_state(AppState::Explore)),
                explore_hud.run_if(in_state(AppState::Explore)),
                combat_keys.run_if(in_state(AppState::Combat)),
                combat_buttons.run_if(in_state(AppState::Combat)),
                combat_hud.run_if(in_state(AppState::Combat)),
                foe_pulse.run_if(in_state(AppState::Combat)),
                ember_drift,
                end_keys.run_if(in_state(AppState::End)),
            )
                .chain(),
        )
        .run();
}

// ---------------------------------------------------------------------------
// Headless unit tests: host-layer flow (map gen, HUD text, hero turns) with
// no window, no GPU, no audio device.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod host_tests {
    use super::*;

    fn test_session(seed: u64) -> Session {
        let game = Game::new(seed);
        let kinds = kinds_or_current(&game);
        let map = GenMap::generate(seed, 0, &kinds);
        Session {
            game,
            log: Vec::new(),
            player: IVec2::new(MAP_W / 2, MAP_H / 2),
            map,
            song_idx: 1,
        }
    }

    #[test]
    fn map_is_bounded_and_walkable() {
        let m = GenMap::generate(7, 0, &[MonsterKind::Goblin]);
        // Pillars never spawn within 3 of the center, so it stays walkable.
        assert!(m.walkable(IVec2::new(MAP_W / 2, MAP_H / 2)));
        assert!(!m.walkable(IVec2::new(-1, 0)));
        assert!(!m.walkable(IVec2::new(MAP_W, MAP_H)));
        assert!(!m.walkable(IVec2::new(0, 0))); // border
        assert!(m.stairs.x > 0 && m.stairs.y > 0 && m.stairs.x < MAP_W && m.stairs.y < MAP_H);
        // Deterministic.
        let m2 = GenMap::generate(7, 0, &[MonsterKind::Goblin]);
        assert_eq!(m.pillars, m2.pillars);
        assert_eq!(m.stairs, m2.stairs);
    }

    #[test]
    fn iso_projection_is_centered_and_monotone() {
        let o = iso(IVec2::new(0, 0));
        assert!((o.x).abs() < f32::EPSILON);
        let a = iso(IVec2::new(3, 1));
        let b = iso(IVec2::new(4, 1));
        assert!(b.x > a.x && b.y < a.y);
    }

    #[test]
    fn hud_lines_render_with_and_without_foe() {
        let s = test_session(7);
        assert!(hero_line(&s.game).contains("HP"));
        assert!(foe_line(&s.game).contains("Harmony"));
        let mut gone = s.game.clone();
        gone.current = None;
        assert!(foe_line(&gone).contains("No foe"));
    }

    #[test]
    fn hero_turn_through_host_backend_advances_game() {
        let mut session = test_session(11);
        let backend = BackendRes(Mutex::new(Backend::Native));
        let bus = SoundBus::spawn();
        let mut next = NextState::<AppState>::default();
        let turns_before = session.game.hero.turns;
        do_hero_action(&mut session, &backend, &bus, Action::Strike, &mut next);
        assert!(session.game.hero.turns > turns_before);
        assert!(!session.log.is_empty());
    }

    #[test]
    fn log_ring_is_bounded() {
        let mut s = test_session(3);
        for i in 0..100 {
            push_log(&mut s, format!("line {i}"));
        }
        assert_eq!(s.log.len(), 40);
    }
}
